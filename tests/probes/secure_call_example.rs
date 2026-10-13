//! Isolated initial secure calls only; no personal accounts, CLI passwords or devices.
use rust_microsip_media::Codec;
use rust_microsip_telephony::{
    RegistrationConfig, Transport,
    g0_calls::{
        CallProbeError, MediaMeasurement,
        secure::{
            CertificateRejection, NegativeOffer, probe_rejected_offer, probe_secure_call_pair,
            probe_tls_certificate_rejection,
        },
    },
};
use secrecy::{ExposeSecret, SecretString};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    net::SocketAddr,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::Path,
    time::Duration,
};
use tokio_util::sync::CancellationToken;

fn private_file(path: &Path, max: u64) -> Result<Vec<u8>, CallProbeError> {
    let root = fs::symlink_metadata("/lab").map_err(|_| CallProbeError::InvalidConfig)?;
    if !root.is_dir() || root.mode() & 0o777 != 0o700 {
        return Err(CallProbeError::InvalidConfig);
    }
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|_| CallProbeError::InvalidConfig)?;
    let info = file.metadata().map_err(|_| CallProbeError::InvalidConfig)?;
    if !info.is_file()
        || info.mode() & 0o777 != 0o600
        || info.uid() != root.uid()
        || info.len() > max
    {
        return Err(CallProbeError::InvalidConfig);
    }
    let mut bytes = Vec::new();
    file.take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| CallProbeError::InvalidConfig)?;
    if bytes.len() as u64 > max {
        return Err(CallProbeError::InvalidConfig);
    }
    Ok(bytes)
}
fn measurement(m: MediaMeasurement) -> serde_json::Value {
    serde_json::json!({"sent_packets":m.sent_packets,"received_packets":m.received_packets,
        "rms":m.rms,"remote_to_own_power_ratio":m.remote_to_own_power_ratio,"lost_packets":m.lost_packets,"late_packets":m.late_packets})
}
fn control_measurement(
    m: rust_microsip_telephony::g0_calls::secure::ControlMeasurement,
) -> serde_json::Value {
    serde_json::json!({"sent_reports":m.sent_reports,"received_reports":m.received_reports,"peer_echoed_sender_reports":m.peer_echoed_sender_reports,"peer_highest_sequence":m.peer_highest_sequence,"authenticated_sources":m.authenticated_sources,"packet_negatives_verified":m.packet_negatives_verified})
}
async fn run() -> Result<(), CallProbeError> {
    let raw = zeroize::Zeroizing::new(private_file(Path::new("/lab/credentials.json"), 8192)?);
    let accounts: BTreeMap<String, SecretString> =
        serde_json::from_slice::<BTreeMap<String, String>>(&raw)
            .map_err(|_| CallProbeError::InvalidConfig)?
            .into_iter()
            .map(|(k, v)| (k, SecretString::from(v)))
            .collect();
    let ca = private_file(Path::new("/lab/ca.crt"), 256 * 1024)?;
    let ip = tokio::net::lookup_host(("pbx", 5060))
        .await
        .map_err(|_| CallProbeError::Signaling)?
        .find(|a| a.is_ipv4())
        .ok_or(CallProbeError::Signaling)?
        .ip();
    let mut rows = Vec::new();
    for (label, first, second, port) in [("tls", "1201", "1202", 5061)] {
        for codec in [Codec::Pcma, Codec::Pcmu] {
            for reverse in [false, true] {
                let config = |name: &str| -> Result<RegistrationConfig, CallProbeError> {
                    let transport = match label {
                        "udp" => Transport::Udp,
                        "tcp" => Transport::Tcp,
                        _ => Transport::Tls {
                            ca_pem: ca.clone(),
                            server_name: "pbx".into(),
                        },
                    };
                    RegistrationConfig::new(
                        SocketAddr::new(ip, port),
                        transport,
                        name.into(),
                        SecretString::from(
                            accounts
                                .get(name)
                                .ok_or(CallProbeError::InvalidConfig)?
                                .expose_secret(),
                        ),
                        Duration::from_secs(30),
                    )
                    .map_err(|_| CallProbeError::InvalidConfig)
                };
                let result = probe_secure_call_pair(
                    config(first)?,
                    config(second)?,
                    codec,
                    reverse,
                    CancellationToken::new(),
                )
                .await?;
                let controls = serde_json::json!({"first":control_measurement(result.first_control),"second":control_measurement(result.second_control)});
                let result = result.call;
                let row = serde_json::json!({"transport":result.transport,"codec":result.codec,"reverse":result.reverse,
                "first":measurement(result.first),"second":measurement(result.second),"control":controls,"ack_and_termination_verified":result.ack_and_termination_verified,
                "bye_200_verified":result.bye_200_verified,"unregistered":result.unregistered});
                // Numeric, non-sensitive progress only; never log SIP, SDP or samples.
                println!("{}", serde_json::json!({"call_completed":row}));
                rows.push(row);
            }
        }
    }
    for (label, kind) in [
        ("plaintext", NegativeOffer::Plaintext),
        ("missing_crypto", NegativeOffer::MissingCrypto),
        ("unsupported_suite", NegativeOffer::UnsupportedSuite),
    ] {
        let config = |name: &str| -> Result<RegistrationConfig, CallProbeError> {
            RegistrationConfig::new(
                SocketAddr::new(ip, 5061),
                Transport::Tls {
                    ca_pem: ca.clone(),
                    server_name: "pbx".into(),
                },
                name.into(),
                SecretString::from(
                    accounts
                        .get(name)
                        .ok_or(CallProbeError::InvalidConfig)?
                        .expose_secret(),
                ),
                Duration::from_secs(30),
            )
            .map_err(|_| CallProbeError::InvalidConfig)
        };
        probe_rejected_offer(
            config("1201")?,
            config("1202")?,
            kind,
            CancellationToken::new(),
        )
        .await?;
        println!(
            "{}",
            serde_json::json!({"negative_offer":label,"response":488,"unregistered":true,"callee_invite_observed":false})
        );
    }
    let untrusted = private_file(Path::new("/lab/untrusted-ca.crt"), 256 * 1024)?;
    for (label, trust, identity, expected) in [
        (
            "untrusted_ca",
            untrusted,
            "pbx",
            CertificateRejection::UntrustedAuthority,
        ),
        (
            "wrong_hostname",
            ca.clone(),
            "wrong.lab.invalid",
            CertificateRejection::WrongIdentity,
        ),
    ] {
        let config = |name: &str,
                      anchor: Vec<u8>,
                      server_name: &str|
         -> Result<RegistrationConfig, CallProbeError> {
            RegistrationConfig::new(
                SocketAddr::new(ip, 5061),
                Transport::Tls {
                    ca_pem: anchor,
                    server_name: server_name.into(),
                },
                name.into(),
                SecretString::from(
                    accounts
                        .get(name)
                        .ok_or(CallProbeError::InvalidConfig)?
                        .expose_secret(),
                ),
                Duration::from_secs(30),
            )
            .map_err(|_| CallProbeError::InvalidConfig)
        };
        probe_tls_certificate_rejection(
            config("1201", trust, identity)?,
            expected,
            CancellationToken::new(),
        )
        .await?;
        println!(
            "{}",
            serde_json::json!({"negative_tls":label,"rejected_before_call":true,"typed_certificate_error_verified":true,"sip_endpoint_constructed":false})
        );
    }
    println!(
        "{}",
        serde_json::json!({"schema_version":1,"fixture":"two-rust-endpoints-initial-sdes-srtp-srtcp",
        "calls":rows,"seconds_per_call":12,"endpoint_processes":1,"distinct_signaling_and_media_sockets":true,
        "synthetic_sources_hz":[440,880],"plaintext_rtp_explicit":false,"initial_srtp_tested":true,"per_endpoint_actual_packet_negatives":["wrong_key","tag_tamper","sequence_tamper","clear_rtp","replay"],"srtcp_tested":true,"rekey_tested":false,"secure_hold_transfer_tested":false,
        "human_voice_tested":false,"release_artifact_tested":false,"g0_passed":false})
    );
    Ok(())
}
#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() {
    if std::env::args_os().len() != 1 {
        eprintln!("This isolated call probe accepts no arguments or personal account settings.");
        std::process::exit(2);
    }
    if let Err(error) = run().await {
        eprintln!("G0 call probe failed: {error}.");
        std::process::exit(1);
    }
}
