//! Fixed disposable IVR cases only; never personal accounts or devices.
use rust_microsip_media::Codec;
use rust_microsip_telephony::{
    RegistrationConfig, Transport,
    g0_calls::{
        CallProbeError,
        controls::{IvrCase, probe_ivr},
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
    let mut bytes = Vec::with_capacity((max + 1) as usize);
    file.take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| CallProbeError::InvalidConfig)?;
    if bytes.len() as u64 > max {
        return Err(CallProbeError::InvalidConfig);
    }
    Ok(bytes)
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
    for (label, user, port) in [
        ("udp", "1001", 5060),
        ("tcp", "1101", 5060),
        ("tls", "1301", 5061),
    ] {
        for codec in [Codec::Pcma, Codec::Pcmu] {
            for case in [IvrCase::Accepted, IvrCase::Rejected] {
                let transport = match label {
                    "udp" => Transport::Udp,
                    "tcp" => Transport::Tcp,
                    _ => Transport::Tls {
                        ca_pem: ca.clone(),
                        server_name: "pbx".into(),
                    },
                };
                let config = RegistrationConfig::new(
                    SocketAddr::new(ip, port),
                    transport,
                    user.into(),
                    SecretString::from(
                        accounts
                            .get(user)
                            .ok_or(CallProbeError::InvalidConfig)?
                            .expose_secret(),
                    ),
                    Duration::from_secs(30),
                )
                .map_err(|_| CallProbeError::InvalidConfig)?;
                let report = probe_ivr(config, codec, case, CancellationToken::new()).await?;
                let expected_hz = match case {
                    IvrCase::Accepted => 1000,
                    IvrCase::Rejected => 400,
                };
                let row = serde_json::json!({"transport":report.transport,"codec":report.codec,"expected_ivr_hz":expected_hz,
                    "negotiated_event_pt":report.event_payload_type,"event_packets_sent":report.event_packets_sent,
                    "received_audio_packets":report.received_audio_packets,"rms":report.rms,"expected_to_other_tone_ratio":report.expected_to_other_tone_ratio,
                    "signaling_completed":report.signaling_completed});
                println!("{}", serde_json::json!({"ivr_case_completed":row}));
                rows.push(row);
            }
        }
    }
    println!(
        "{}",
        serde_json::json!({"schema_version":1,"fixture":"rust-rfc4733-ivr","cases":rows,
        "positive_and_negative_ivr_cases":true,"tone_measurement_seconds":2,"plaintext_rtp_explicit":true,
        "hold_tested":false,"refer_tested":false,"srtp_tested":false,"human_voice_tested":false,
        "release_artifact_tested":false,"g0_passed":false})
    );
    Ok(())
}
#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() {
    if std::env::args_os().len() != 1 {
        eprintln!("This isolated IVR probe accepts no arguments or personal account settings.");
        std::process::exit(2);
    }
    if let Err(error) = run().await {
        eprintln!("G0 IVR probe failed: {error}.");
        std::process::exit(1);
    }
}
