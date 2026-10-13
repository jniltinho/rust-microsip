//! Experimental SIP adapter for the G0 interoperability proof.
//!
//! [`probe_registration`] performs a bounded register/refresh/unregister cycle
//! against an explicitly selected registrar. It does not read HOME/configuration,
//! enable a microphone, make calls, or approve G0. Upstream types remain private.
use rsipstack::{
    EndpointBuilder,
    dialog::{authenticate::Credential, registration::Registration},
    sip::{HeadersExt, Method, SipMessage, StatusCode, Uri},
    transaction::endpoint::MessageInspector,
    transport::{SipAddr, TransportLayer, tls::TlsConfig, udp::UdpConnection},
};
use secrecy::{ExposeSecret, SecretString};
use std::{
    fmt,
    net::SocketAddr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, Ordering},
    },
    time::Duration,
};
use thiserror::Error;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use zeroize::Zeroize;

/// Explicit signaling transport. Insecure modes are for deliberate lab use.
pub enum Transport {
    /// Unencrypted UDP signaling; no implied secure media.
    Udp,
    /// Unencrypted TCP signaling; no implied secure media.
    Tcp,
    /// TLS with an explicit trust anchor and independently checked identity.
    Tls {
        /// PEM CA bytes, bounded at configuration validation.
        ca_pem: Vec<u8>,
        /// DNS identity or IP literal to validate against the certificate.
        server_name: String,
    },
}

impl Transport {
    /// Return a non-sensitive static transport label.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Udp => "udp",
            Self::Tcp => "tcp",
            Self::Tls { .. } => "tls",
        }
    }
}

impl fmt::Debug for Transport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Validated, secret-redacted inputs for one experimental registration cycle.
///
/// This is not the release configuration schema or a multi-account manager.
pub struct RegistrationConfig {
    address: SocketAddr,
    transport: Transport,
    username: String,
    password: SecretString,
    deadline: Duration,
}

impl fmt::Debug for RegistrationConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RegistrationConfig")
            .field("transport", &self.transport)
            .field("identity", &"[REDACTED]")
            .field("deadline", &self.deadline)
            .finish_non_exhaustive()
    }
}

impl RegistrationConfig {
    /// Validate explicit adapter inputs without filesystem or network access.
    ///
    /// # Errors
    /// Returns [`RegistrationError::InvalidConfig`] for an empty/oversized secret,
    /// unsafe username/identity, missing TLS CA, zero port, or an unbounded deadline.
    pub fn new(
        address: SocketAddr,
        transport: Transport,
        username: String,
        password: SecretString,
        deadline: Duration,
    ) -> Result<Self, RegistrationError> {
        if address.port() == 0
            || address.ip().is_unspecified()
            || address.ip().is_multicast()
            || username.is_empty()
            || username.len() > 128
            || !username
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            || password.expose_secret().is_empty()
            || password.expose_secret().len() > 1024
            || deadline.is_zero()
            || deadline > Duration::from_secs(30)
        {
            return Err(RegistrationError::InvalidConfig);
        }
        if let Transport::Tls {
            ca_pem,
            server_name,
        } = &transport
            && (ca_pem.is_empty()
                || ca_pem.len() > 256 * 1024
                || server_name.is_empty()
                || server_name.len() > 253
                || !server_name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b".-:".contains(&b)))
        {
            return Err(RegistrationError::InvalidConfig);
        }
        Ok(Self {
            address,
            transport,
            username,
            password,
            deadline,
        })
    }

    /// Change the bounded operation deadline without opening a connection.
    ///
    /// # Errors
    /// Returns [`RegistrationError::InvalidConfig`] for zero or more than 30 seconds.
    pub fn set_deadline(&mut self, deadline: Duration) -> Result<(), RegistrationError> {
        if deadline.is_zero() || deadline > Duration::from_secs(30) {
            return Err(RegistrationError::InvalidConfig);
        }
        self.deadline = deadline;
        Ok(())
    }
}

/// Sanitized failure categories; upstream auth/network payloads are never exposed.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum RegistrationError {
    /// Explicit adapter inputs failed validation.
    #[error("invalid registration configuration")]
    InvalidConfig,
    /// The caller cancelled the operation.
    #[error("registration cancelled")]
    Cancelled,
    /// The complete probe exceeded its finite deadline.
    #[error("registration deadline exceeded")]
    Timeout,
    /// A connection, TLS validation, or protocol operation failed.
    #[error("registration transport or protocol failed")]
    Transport,
    /// A credential challenge remained rejected after the bounded auth attempt.
    #[error("registration authentication rejected")]
    Authentication,
    /// Registrar returned a non-success status, containing no untrusted text.
    #[error("registrar rejected registration with status {0}")]
    Rejected(u16),
}

/// Non-sensitive evidence for one completed register/refresh/unregister probe.
#[derive(Debug)]
pub struct RegistrationReport {
    /// Static transport label; no account, address or credentials.
    pub transport: &'static str,
    /// Number of observed 401/407 challenge responses across the cycle.
    pub challenges: u32,
    /// Number of successfully completed explicit refresh requests.
    pub refreshes: u8,
    /// Whether the final authenticated unregistration returned 200.
    pub unregistered: bool,
    /// Inspected REGISTERs matched the concrete transport-reported local socket.
    /// This alone does not prove external reachability or independent TCP/TLS peer evidence.
    pub contact_verified: bool,
    /// Inspected REGISTERs retained one nonempty, bounded Call-ID.
    pub call_id_stable: bool,
    /// Completed register/refresh/unregister phases used strictly increasing CSeq.
    pub cseq_increased: bool,
}

struct WireEvidence {
    expected_contact: SocketAddr,
    call_id: Option<String>,
    last_cseq: u32,
    messages: u8,
    valid: bool,
}
impl WireEvidence {
    fn observe(&mut self, call_id: &str, cseq: u32, contact: SocketAddr) {
        self.messages = self.messages.saturating_add(1);
        self.valid &= self.messages <= 64
            && !call_id.is_empty()
            && call_id.len() <= 256
            && cseq > 0
            && cseq >= self.last_cseq
            && contact == self.expected_contact;
        if let Some(first) = &self.call_id {
            self.valid &= first == call_id;
        } else if call_id.len() <= 256 {
            self.call_id = Some(call_id.into());
        }
        self.last_cseq = cseq;
    }
}

struct ProbeInspector {
    challenges: Arc<AtomicU32>,
    wire: Arc<Mutex<WireEvidence>>,
}
impl MessageInspector for ProbeInspector {
    fn before_send(&self, msg: SipMessage, _: Option<&SipAddr>) -> SipMessage {
        if let SipMessage::Request(request) = &msg
            && request.method == Method::Register
            && let Ok(mut wire) = self.wire.lock()
        {
            let fields = (|| {
                let contact =
                    rsipstack::sip::typed::Contact::parse(request.contact_header().ok()?.value())
                        .ok()?;
                let address = contact
                    .uri
                    .host_with_port
                    .to_string()
                    .parse::<SocketAddr>()
                    .ok()?;
                let seq = rsipstack::sip::typed::CSeq::parse(request.cseq_header().ok()?.value())
                    .ok()?
                    .seq;
                Some((request.call_id_header().ok()?.value(), seq, address))
            })();
            if let Some((call_id, seq, address)) = fields {
                wire.observe(call_id, seq, address);
            } else {
                wire.valid = false;
            }
        }
        msg
    }
    fn after_received(&self, msg: SipMessage, _: Option<&SipAddr>) -> SipMessage {
        if matches!(&msg, SipMessage::Response(r) if matches!(r.status_code, StatusCode::Unauthorized | StatusCode::ProxyAuthenticationRequired))
        {
            self.challenges.fetch_add(1, Ordering::Relaxed);
        }
        msg
    }
}

struct OwnedRegistration(Registration);
impl Drop for OwnedRegistration {
    fn drop(&mut self) {
        // This adapter-owned copy is cleared on success, error and cancellation.
        // Upstream temporary copies still need a separate lifetime audit.
        if let Some(credential) = &mut self.0.credential {
            credential.password.zeroize();
        }
    }
}

struct ProbeTasks {
    stop: CancellationToken,
    tasks: JoinSet<()>,
}
impl Drop for ProbeTasks {
    fn drop(&mut self) {
        self.stop.cancel();
        self.tasks.abort_all();
    }
}

/// Probe real SIP registration, an explicit refresh and unregistration.
///
/// No automatic transport downgrade, account discovery, tracing subscriber or
/// logging is installed. TLS always uses rustls's normal CA/hostname verifier.
/// Dropping/cancelling the future cancels and aborts its owned background tasks.
/// This probe is for an isolated G0 lab, not full renewal/backoff acceptance.
///
/// # Errors
/// Returns a sanitized [`RegistrationError`]. No success is reported until
/// registration, refresh and unregistration all returned 200 before the deadline.
pub async fn probe_registration(
    config: RegistrationConfig,
    cancel: CancellationToken,
) -> Result<RegistrationReport, RegistrationError> {
    if cancel.is_cancelled() {
        return Err(RegistrationError::Cancelled);
    }
    let mut owner = ProbeTasks {
        stop: cancel.child_token(),
        tasks: JoinSet::new(),
    };
    let deadline = config.deadline;
    tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(RegistrationError::Cancelled),
        result = tokio::time::timeout(deadline, probe_inner(config, &mut owner)) =>
            result.map_err(|_| RegistrationError::Timeout)?,
    }
}

async fn probe_inner(
    config: RegistrationConfig,
    owner: &mut ProbeTasks,
) -> Result<RegistrationReport, RegistrationError> {
    let label = config.transport.as_str();
    let transport = TransportLayer::new(owner.stop.clone());
    // Ask the kernel for this route's concrete local IP without sending data.
    let local = if config.address.is_ipv6() {
        "[::]:0"
    } else {
        "0.0.0.0:0"
    };
    let route = tokio::net::UdpSocket::bind(local)
        .await
        .map_err(|_| RegistrationError::Transport)?;
    route
        .connect(config.address)
        .await
        .map_err(|_| RegistrationError::Transport)?;
    let local_ip = route
        .local_addr()
        .map_err(|_| RegistrationError::Transport)?
        .ip();
    drop(route);
    let udp = UdpConnection::create_connection(
        SocketAddr::new(local_ip, 0),
        None,
        Some(owner.stop.child_token()),
    )
    .await
    .map_err(|_| RegistrationError::Transport)?;
    transport.add_transport(udp.into());
    if let Transport::Tls {
        ca_pem,
        server_name,
    } = config.transport
    {
        // Never supply a custom certificate verifier or register an insecure backend.
        transport.set_tls_config(TlsConfig {
            ca_certs: Some(ca_pem),
            sni_hostname: Some(server_name),
            ..Default::default()
        });
    }
    let server: Uri = format!("sip:{};transport={label}", config.address)
        .parse()
        .map_err(|_| RegistrationError::InvalidConfig)?;
    let destination = SipAddr::try_from(&server).map_err(|_| RegistrationError::Transport)?;
    // Eagerly validate TLS before constructing auth or sending any SIP bytes.
    let (connection, _) = transport
        .lookup(&destination, None)
        .await
        .map_err(|_| RegistrationError::Transport)?;
    let local_address = connection
        .get_addr()
        .get_socketaddr()
        .map_err(|_| RegistrationError::Transport)?;
    if local_address.ip().is_unspecified()
        || local_address.port() == 0
        || local_address == config.address
        || local_address.ip().is_loopback() != config.address.ip().is_loopback()
    {
        return Err(RegistrationError::Transport);
    }
    let contact: Uri = format!(
        "sip:{}@{};transport={label}",
        config.username,
        connection.get_addr().addr
    )
    .parse()
    .map_err(|_| RegistrationError::Transport)?;
    let challenges = Arc::new(AtomicU32::new(0));
    let wire = Arc::new(Mutex::new(WireEvidence {
        expected_contact: local_address,
        call_id: None,
        last_cseq: 0,
        messages: 0,
        valid: true,
    }));
    let endpoint = EndpointBuilder::new()
        .with_cancel_token(owner.stop.clone())
        .with_transport_layer(transport)
        .with_user_agent("rust-microsip G0 probe")
        .with_inspector(Box::new(ProbeInspector {
            challenges: challenges.clone(),
            wire: wire.clone(),
        }))
        .with_allows(vec![Method::Register, Method::Options])
        .build();
    let inner = endpoint.inner.clone();
    let stopped = owner.stop.clone();
    owner.tasks.spawn(async move {
        if inner.serve().await.is_err() {
            stopped.cancel();
        }
    });
    let mut incoming = endpoint
        .incoming_transactions()
        .map_err(|_| RegistrationError::Transport)?;
    let stopped = owner.stop.clone();
    owner.tasks.spawn(async move {
        loop {
            tokio::select! {
                biased;
                _ = stopped.cancelled() => break,
                transaction = incoming.recv() => {
                    let Some(mut transaction) = transaction else { break };
                    let status = if transaction.original.method == Method::Options { StatusCode::OK } else { StatusCode::NotImplemented };
                    let _ = tokio::time::timeout(Duration::from_secs(1), transaction.reply(status)).await;
                }
            }
        }
    });
    let credential = Credential {
        username: config.username,
        password: config.password.expose_secret().into(),
        realm: None,
        auth_username: None,
    };
    let mut registration =
        OwnedRegistration(Registration::new(endpoint.inner.clone(), Some(credential)));
    registration.0.contact = Some(rsipstack::sip::typed::Contact {
        display_name: None,
        uri: contact,
        params: vec![],
    });
    let outcome = async {
        let mut previous_phase = 0;
        for expires in [120, 120, 0] {
            let response = registration
                .0
                .register(server.clone(), Some(expires))
                .await
                .map_err(|_| RegistrationError::Transport)?;
            match response.status_code {
                StatusCode::OK => (),
                StatusCode::Unauthorized
                | StatusCode::ProxyAuthenticationRequired
                | StatusCode::Forbidden => return Err(RegistrationError::Authentication),
                status => return Err(RegistrationError::Rejected(status.into())),
            }
            // Equal CSeq is allowed only for a retransmission within a phase;
            // each completed operation must advance it while retaining Call-ID.
            let evidence = wire.lock().map_err(|_| RegistrationError::Transport)?;
            if !evidence.valid || evidence.messages == 0 || evidence.last_cseq <= previous_phase {
                return Err(RegistrationError::Transport);
            }
            previous_phase = evidence.last_cseq;
        }
        Ok(RegistrationReport {
            transport: label,
            challenges: challenges.load(Ordering::Relaxed),
            refreshes: 1,
            unregistered: true,
            contact_verified: true,
            call_id_stable: true,
            cseq_increased: true,
        })
    }
    .await;
    endpoint.shutdown();
    outcome
}

#[cfg(test)]
mod tests {
    use super::WireEvidence;

    #[test]
    fn wire_evidence_rejects_contact_identity_and_sequence_changes()
    -> Result<(), std::net::AddrParseError> {
        for (id, seq, address) in [
            ("changed", 2, "127.0.0.1:1234"),
            ("same", 1, "127.0.0.1:1234"),
            ("same", 3, "0.0.0.0:1234"),
        ] {
            let mut wire = WireEvidence {
                expected_contact: "127.0.0.1:1234".parse()?,
                call_id: None,
                last_cseq: 0,
                messages: 0,
                valid: true,
            };
            wire.observe("same", 2, "127.0.0.1:1234".parse()?);
            wire.observe(id, seq, address.parse()?);
            assert!(!wire.valid);
        }
        Ok(())
    }
}
