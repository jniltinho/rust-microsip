//! Registration configuration and bounded failure contracts.
use rust_microsip_telephony::{
    RegistrationConfig, RegistrationError, Transport, probe_registration,
};
use secrecy::SecretString;
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    time::Duration,
};
use tokio_util::sync::CancellationToken;

fn config(transport: Transport) -> Result<RegistrationConfig, RegistrationError> {
    RegistrationConfig::new(
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 5060),
        transport,
        "1001".into(),
        SecretString::from("fictional-private-marker"),
        Duration::from_secs(2),
    )
}

#[test]
fn valid_lab_configuration_is_redacted() -> Result<(), RegistrationError> {
    let cfg = config(Transport::Udp)?;
    let debug = format!("{cfg:?}");
    assert!(!debug.contains("fictional-private-marker"));
    assert!(!debug.contains("1001"));
    Ok(())
}

#[test]
#[expect(
    clippy::unwrap_used,
    reason = "fixed socket-address literals in test fixtures"
)]
fn rejects_header_injection_empty_secret_and_unbounded_deadline() {
    for user in [
        "",
        "1001\r\nInjected: value",
        "alice@other.invalid",
        &"a".repeat(129),
    ] {
        assert!(
            RegistrationConfig::new(
                "127.0.0.1:5060".parse().unwrap(),
                Transport::Udp,
                user.into(),
                SecretString::from("fictional"),
                Duration::from_secs(2)
            )
            .is_err()
        );
    }
    assert!(
        RegistrationConfig::new(
            "127.0.0.1:5060".parse().unwrap(),
            Transport::Udp,
            "1001".into(),
            SecretString::from(""),
            Duration::from_secs(2)
        )
        .is_err()
    );
    for deadline in [Duration::ZERO, Duration::from_secs(31)] {
        assert!(
            RegistrationConfig::new(
                "127.0.0.1:5060".parse().unwrap(),
                Transport::Udp,
                "1001".into(),
                SecretString::from("fictional"),
                deadline
            )
            .is_err()
        );
    }
}

#[test]
fn tls_requires_nonempty_ca_and_valid_server_identity() {
    for (ca, name) in [(vec![], "pbx"), (vec![1], "bad\r\nname"), (vec![1], "")] {
        assert!(
            config(Transport::Tls {
                ca_pem: ca,
                server_name: name.into()
            })
            .is_err()
        );
    }
}

#[tokio::test]
async fn cancellation_before_start_opens_no_endpoint() -> Result<(), RegistrationError> {
    let stop = CancellationToken::new();
    stop.cancel();
    assert!(matches!(
        probe_registration(config(Transport::Udp)?, stop).await,
        Err(RegistrationError::Cancelled)
    ));
    Ok(())
}

#[tokio::test]
async fn silent_registrar_has_a_finite_deadline() -> Result<(), Box<dyn std::error::Error>> {
    let sink = tokio::net::UdpSocket::bind("127.0.0.1:0").await?;
    let mut cfg = RegistrationConfig::new(
        sink.local_addr()?,
        Transport::Udp,
        "1001".into(),
        SecretString::from("fictional"),
        Duration::from_secs(2),
    )?;
    cfg.set_deadline(Duration::from_millis(150))?;
    let start = tokio::time::Instant::now();
    let result = probe_registration(cfg, CancellationToken::new()).await;
    assert!(matches!(
        result,
        Err(RegistrationError::Timeout | RegistrationError::Transport)
    ));
    assert!(start.elapsed() < Duration::from_secs(2));
    Ok(())
}

#[tokio::test]
async fn concrete_udp_contact_and_inflight_cancellation() -> Result<(), Box<dyn std::error::Error>>
{
    let sink = tokio::net::UdpSocket::bind("127.0.0.1:0").await?;
    let cfg = RegistrationConfig::new(
        sink.local_addr()?,
        Transport::Udp,
        "1001".into(),
        SecretString::from("fictional"),
        Duration::from_secs(2),
    )?;
    let cancel = CancellationToken::new();
    let probe = tokio::spawn(probe_registration(cfg, cancel.clone()));
    let mut packet = [0; 8192];
    let (length, source) =
        tokio::time::timeout(Duration::from_secs(1), sink.recv_from(&mut packet)).await??;
    let request = std::str::from_utf8(&packet[..length])?;
    assert!(
        request
            .to_ascii_lowercase()
            .contains(&format!("contact: <sip:1001@{source};transport=udp>"))
    );
    assert!(!request.contains("@0.0.0.0:"));
    cancel.cancel();
    assert!(matches!(
        tokio::time::timeout(Duration::from_millis(250), probe).await??,
        Err(RegistrationError::Cancelled)
    ));
    Ok(())
}
