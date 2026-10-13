//! SIP stack for one account: transports, endpoint, dialog layer, registration
//! and dispatch of incoming transactions to the engine actor.

use super::Internal;
use rsipstack::{
    EndpointBuilder,
    dialog::{
        DialogId, authenticate::Credential, dialog::Dialog, dialog::DialogStateSender,
        dialog_layer::DialogLayer, registration::Registration,
    },
    sip::{Method, StatusCode, Uri},
    transaction::{endpoint::Endpoint, transaction::Transaction},
    transport::{SipAddr, SipConnection, TransportLayer, tls::TlsConfig, udp::UdpConnection},
};
use rust_microsip_core::{AccountSettings, RegistrationFailure, Transport};
use secrecy::{ExposeSecret, SecretString};
use std::{
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::Duration,
};
use tokio::{net::UdpSocket, sync::mpsc};
use tokio_util::sync::CancellationToken;

/// Public CA bundles probed in order when no custom CA is configured.
const SYSTEM_CA_BUNDLES: [&str; 4] = [
    "/etc/ssl/certs/ca-certificates.crt",
    "/etc/pki/tls/certs/ca-bundle.crt",
    "/etc/ssl/ca-bundle.pem",
    "/etc/ssl/cert.pem",
];
const MAX_CA_BYTES: u64 = 4 * 1024 * 1024;

fn system_roots() -> Option<Vec<u8>> {
    SYSTEM_CA_BUNDLES.iter().find_map(|path| {
        let meta = std::fs::metadata(path).ok()?;
        (meta.is_file() && meta.len() <= MAX_CA_BYTES)
            .then(|| std::fs::read(path).ok())
            .flatten()
    })
}

/// Running stack; dropping the token stops every task it spawned.
pub(super) struct Stack {
    pub stop: CancellationToken,
    pub endpoint: Endpoint,
    pub layer: Arc<DialogLayer>,
    pub contact: Uri,
    pub destination: SipAddr,
    pub local_ip: IpAddr,
    pub states: DialogStateSender,
    pub credential: Credential,
    pub settings: AccountSettings,
}
impl Stack {
    /// Resolve, connect and serve; does not register.
    pub(super) async fn start(
        settings: AccountSettings,
        password: &SecretString,
        custom_ca: Option<Vec<u8>>,
        internal: mpsc::Sender<Internal>,
    ) -> Result<Self, RegistrationFailure> {
        let server = tokio::time::timeout(
            Duration::from_secs(10),
            tokio::net::lookup_host((settings.server.as_str(), settings.port)),
        )
        .await
        .map_err(|_| RegistrationFailure::Timeout)?
        .map_err(|_| RegistrationFailure::Network)?
        .min_by_key(|a| !a.is_ipv4())
        .ok_or(RegistrationFailure::Network)?;
        let stop = CancellationToken::new();
        let route = UdpSocket::bind(if server.is_ipv4() {
            "0.0.0.0:0"
        } else {
            "[::]:0"
        })
        .await
        .map_err(|_| RegistrationFailure::Network)?;
        route
            .connect(server)
            .await
            .map_err(|_| RegistrationFailure::Network)?;
        let local_ip = route
            .local_addr()
            .map_err(|_| RegistrationFailure::Network)?
            .ip();
        drop(route);
        let transport = TransportLayer::new(stop.clone());
        // Only the configured transport listens: a TLS account must not
        // accept plaintext UDP requests that could carry SDES keys in clear.
        if settings.transport == Transport::Udp {
            let udp = UdpConnection::create_connection(
                SocketAddr::new(local_ip, 0),
                None,
                Some(stop.child_token()),
            )
            .await
            .map_err(|_| RegistrationFailure::Network)?;
            transport.add_transport(udp.into());
        }
        if settings.transport == Transport::Tls {
            // A configured bundle without any parsable certificate is a trust
            // error, reported as such (it would otherwise trust nothing).
            if let Some(pem) = &custom_ca {
                use rustls::pki_types::{CertificateDer, pem::PemObject};
                let parsed: Result<Vec<_>, _> = CertificateDer::pem_slice_iter(pem).collect();
                if parsed.map_or(true, |certs| certs.is_empty()) {
                    return Err(RegistrationFailure::Certificate);
                }
            }
            transport.set_tls_config(TlsConfig {
                ca_certs: Some(
                    custom_ca
                        .or_else(system_roots)
                        .ok_or(RegistrationFailure::Certificate)?,
                ),
                sni_hostname: Some(settings.server.clone()),
                ..Default::default()
            });
        }
        let label = settings.transport.as_str();
        let target: Uri = format!("sip:{server};transport={label}")
            .parse()
            .map_err(|_| RegistrationFailure::Network)?;
        let destination = SipAddr::try_from(&target).map_err(|_| RegistrationFailure::Network)?;
        let (connection, _) = tokio::time::timeout(
            Duration::from_secs(10),
            transport.lookup(&destination, None),
        )
        .await
        .map_err(|_| RegistrationFailure::Timeout)?
        .map_err(|error| super::classify_connect(&error))?;
        let local = connection
            .get_addr()
            .get_socketaddr()
            .map_err(|_| RegistrationFailure::Network)?;
        let contact: Uri = format!("sip:{}@{local};transport={label}", settings.username)
            .parse()
            .map_err(|_| RegistrationFailure::Network)?;
        let endpoint = EndpointBuilder::new()
            .with_cancel_token(stop.clone())
            .with_transport_layer(transport)
            .with_user_agent(concat!("rust-microsip/", env!("CARGO_PKG_VERSION")))
            .with_allows(vec![
                Method::Register,
                Method::Invite,
                Method::Ack,
                Method::Bye,
                Method::Cancel,
                Method::Options,
                Method::Refer,
                Method::Notify,
            ])
            .build();
        let layer = Arc::new(DialogLayer::new(endpoint.inner.clone()));
        let inner = endpoint.inner.clone();
        let stopped = stop.clone();
        tokio::spawn(async move {
            if inner.serve().await.is_err() {
                stopped.cancel();
            }
        });
        let (states, mut events) = layer.new_dialog_state_channel();
        let forward = internal.clone();
        let stopped = stop.clone();
        tokio::spawn(async move {
            loop {
                let event = tokio::select! {biased; _ = stopped.cancelled() => break, e = events.recv() => e};
                let Some(event) = event else { break };
                if forward
                    .send(Internal::Dialog(Box::new(event)))
                    .await
                    .is_err()
                {
                    break;
                }
            }
        });
        let mut transactions = endpoint
            .incoming_transactions()
            .map_err(|_| RegistrationFailure::Network)?;
        let dispatch_layer = layer.clone();
        let stopped = stop.clone();
        let kind = settings.transport;
        tokio::spawn(async move {
            loop {
                let item = tokio::select! {biased; _ = stopped.cancelled() => break, t = transactions.recv() => t};
                let Some(tx) = item else { break };
                dispatch(&dispatch_layer, tx, &internal, server, kind).await;
            }
        });
        let credential = Credential {
            username: settings.username.clone(),
            password: password.expose_secret().to_owned(),
            realm: None,
            auth_username: settings.auth_username.clone(),
        };
        Ok(Self {
            stop,
            endpoint,
            layer,
            contact,
            destination,
            local_ip,
            states,
            credential,
            settings,
        })
    }

    /// A registration client bound to this stack (REGISTER via the server).
    pub(super) fn registration(&self) -> Result<(Registration, Uri), RegistrationFailure> {
        let mut registration =
            Registration::new(self.endpoint.inner.clone(), Some(self.credential.clone()));
        registration.contact = Some(rsipstack::sip::typed::Contact {
            display_name: None,
            uri: self.contact.clone(),
            params: vec![],
        });
        registration.outbound_proxy = self.destination.get_socketaddr().ok();
        let registrar: Uri = format!(
            "sip:{};transport={}",
            self.settings.domain,
            self.settings.transport.as_str()
        )
        .parse()
        .map_err(|_| RegistrationFailure::Network)?;
        Ok((registration, registrar))
    }

    /// SIP URI for a user part in the account domain.
    pub(super) fn uri(&self, user: &str) -> Option<Uri> {
        // '#' is a URI delimiter; DialTarget allows it for feature codes.
        let user = user.replace('#', "%23");
        format!("sip:{user}@{}", self.settings.domain).parse().ok()
    }
}

async fn dispatch(
    layer: &Arc<DialogLayer>,
    mut tx: Transaction,
    internal: &mpsc::Sender<Internal>,
    peer: SocketAddr,
    kind: Transport,
) {
    // Only the configured registrar/proxy, over the configured transport,
    // may send requests to this UA.
    let from_peer = tx.connection.as_ref().is_some_and(|connection| {
        let transport_ok = matches!(
            (kind, connection),
            (Transport::Udp, SipConnection::Udp(_))
                | (Transport::Tcp, SipConnection::Tcp(_))
                | (Transport::Tls, SipConnection::Tls(_))
        );
        transport_ok
            && connection
                .get_remote_addr()
                .and_then(|a| a.get_socketaddr().ok())
                == Some(peer)
    });
    if !from_peer {
        let _ = tx.reply(StatusCode::Forbidden).await;
        return;
    }
    if let Some(Dialog::Invite(mut dialog)) = layer.match_dialog(&tx) {
        tokio::spawn(async move {
            let _ = dialog.handle(&mut tx).await;
        });
        return;
    }
    match tx.original.method {
        Method::Invite => {
            // The actor decides; it owns the transaction from here on.
            if let Err(error) = internal.try_send(Internal::Incoming(Box::new(tx))) {
                // Engine saturated: never leave an INVITE unanswered.
                if let Internal::Incoming(mut tx) = error.into_inner() {
                    let _ = tx.reply(StatusCode::ServiceUnavailable).await;
                }
            }
        }
        Method::Options => {
            let _ = tx.reply(StatusCode::OK).await;
        }
        Method::Ack => (),
        _ => {
            let _ = tx.reply(StatusCode::CallTransactionDoesNotExist).await;
        }
    }
}

/// Dialog id carried by a dialog state event.
pub(super) fn dialog_id(state: &rsipstack::dialog::dialog::DialogState) -> Option<&DialogId> {
    use rsipstack::dialog::dialog::DialogState as S;
    match state {
        S::Calling(id)
        | S::Trying(id)
        | S::Early(id, _)
        | S::WaitAck(id, _)
        | S::Confirmed(id, _)
        | S::Updated(id, _, _)
        | S::Publish(id, _, _)
        | S::Notify(id, _, _)
        | S::Refer(id, _, _)
        | S::Message(id, _, _)
        | S::Info(id, _, _)
        | S::Options(id, _, _)
        | S::Terminated(id, _) => Some(id),
        #[allow(unreachable_patterns)]
        _ => None,
    }
}
