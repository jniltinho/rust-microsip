//! Fixed disposable-lab calls only; no personal accounts, CLI passwords or devices.
use rust_microsip_media::Codec;
use rust_microsip_telephony::{
    RegistrationConfig, Transport,
    g0_calls::{
        CallProbeError,
        hold::Window,
        transfer::{Destination, probe_transfer_pair},
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
fn measurement(m: Window) -> serde_json::Value {
    serde_json::json!({"sent_packets":m.sent_packets,"received_packets":m.received_packets,"rms":m.rms,
        "remote_to_own_power_ratio":m.remote_to_own_power_ratio,"remote_tone_amplitude":m.remote_tone_amplitude})
}
fn windows(m: [Window; 2]) -> serde_json::Value {
    serde_json::json!([measurement(m[0]), measurement(m[1])])
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
    for (label, first, second, port) in [
        ("udp", "1001", "1002", 5060),
        ("tcp", "1101", "1102", 5060),
        ("tls", "1301", "1302", 5061),
    ] {
        for codec in [Codec::Pcma, Codec::Pcmu] {
            for reverse in [false, true] {
                for destination in [Destination::ReferenceTone, Destination::Missing] {
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
                    let result = probe_transfer_pair(
                        config(first)?,
                        config(second)?,
                        codec,
                        reverse,
                        destination,
                        CancellationToken::new(),
                    )
                    .await?;
                    let row = serde_json::json!({"transport":result.transport,"codec":result.codec,"reverse":result.reverse,
                    "destination":if result.destination==Destination::ReferenceTone {"tone"} else {"missing"},
                    "refer_response":result.refer_response,"refer_cseq":result.refer_cseq,"final_notify_status":result.final_notify_status,
                    "notify_count":result.notify_count,"provisional_notify_count":result.provisional_notify_count,
                    "before":windows(result.before),"restored":result.restored.map(windows),"reference_tone":result.reference_tone.map(measurement),
                    "final_notify_200_reply_emitted":true,"signaling_completed":result.signaling_completed});
                    println!("{}", serde_json::json!({"transfer_case_completed":row}));
                    rows.push(row);
                }
            }
        }
    }
    println!(
        "{}",
        serde_json::json!({"schema_version":1,"fixture":"two-rust-endpoints-refer-notify","cases":rows,
        "voice_window_seconds":2,"endpoint_processes":1,
        "distinct_signaling_and_media_sockets":true,"synthetic_sources_hz":[440,880],"plaintext_rtp_explicit":true,
        "refer_tested":true,"srtp_tested":false,"human_voice_tested":false,"release_artifact_tested":false,"g0_passed":false})
    );
    Ok(())
}
#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() {
    if std::env::args_os().len() != 1 {
        eprintln!(
            "This isolated transfer probe accepts no arguments or personal account settings."
        );
        std::process::exit(2);
    }
    if let Err(error) = run().await {
        eprintln!("G0 transfer probe failed: {error}.");
        std::process::exit(1);
    }
}
