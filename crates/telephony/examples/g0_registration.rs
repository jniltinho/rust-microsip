//! Lab-only real registration probe; never a production account CLI.
use rust_microsip_telephony::{
    RegistrationConfig, RegistrationError, Transport, probe_registration,
};
use secrecy::SecretString;
use std::{
    collections::BTreeMap, fs, net::SocketAddr, os::unix::fs::MetadataExt, path::Path,
    time::Duration,
};
use tokio_util::sync::CancellationToken;

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() {
    if std::env::args_os().len() != 1 {
        eprintln!("This isolated lab probe accepts no arguments or production account settings.");
        std::process::exit(2);
    }
    if run().await.is_err() {
        eprintln!("Rust G0 registration probe failed; inspect only disposable lab readiness.");
        std::process::exit(1);
    }
}

fn private_file(path: &Path, max: u64) -> Result<Vec<u8>, RegistrationError> {
    use std::{io::Read, os::unix::fs::OpenOptionsExt};
    let root = fs::symlink_metadata("/lab").map_err(|_| RegistrationError::InvalidConfig)?;
    if !root.is_dir() || root.mode() & 0o777 != 0o700 {
        return Err(RegistrationError::InvalidConfig);
    }
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|_| RegistrationError::InvalidConfig)?;
    let metadata = file
        .metadata()
        .map_err(|_| RegistrationError::InvalidConfig)?;
    if !metadata.is_file()
        || metadata.mode() & 0o777 != 0o600
        || metadata.uid() != root.uid()
        || metadata.len() > max
    {
        return Err(RegistrationError::InvalidConfig);
    }
    let mut bytes = Vec::new();
    file.take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| RegistrationError::InvalidConfig)?;
    if bytes.len() as u64 > max {
        return Err(RegistrationError::InvalidConfig);
    }
    Ok(bytes)
}

fn make_config(
    address: SocketAddr,
    transport: Transport,
    username: &str,
    password: &str,
) -> Result<RegistrationConfig, RegistrationError> {
    RegistrationConfig::new(
        address,
        transport,
        username.into(),
        SecretString::from(password),
        Duration::from_secs(10),
    )
}

async fn run() -> Result<(), RegistrationError> {
    // Fixed container paths only; no HOME/XDG/dotenv or personal account lookup.
    let raw = zeroize::Zeroizing::new(private_file(Path::new("/lab/credentials.json"), 8192)?);
    let accounts: BTreeMap<String, SecretString> =
        serde_json::from_slice::<BTreeMap<String, String>>(&raw)
            .map_err(|_| RegistrationError::InvalidConfig)?
            .into_iter()
            .map(|(name, password)| (name, SecretString::from(password)))
            .collect();
    let ca = private_file(Path::new("/lab/ca.crt"), 256 * 1024)?;
    let untrusted = private_file(Path::new("/lab/untrusted-ca.crt"), 256 * 1024)?;
    let address = tokio::net::lookup_host(("pbx", 5060))
        .await
        .map_err(|_| RegistrationError::Transport)?
        .find(|a| a.is_ipv4())
        .ok_or(RegistrationError::Transport)?;
    let mut rows = Vec::new();
    for (name, label) in [("1001", "udp"), ("1101", "tcp"), ("1201", "tls")] {
        let transport = match label {
            "udp" => Transport::Udp,
            "tcp" => Transport::Tcp,
            _ => Transport::Tls {
                ca_pem: ca.clone(),
                server_name: "pbx".into(),
            },
        };
        let server = SocketAddr::new(address.ip(), if label == "tls" { 5061 } else { 5060 });
        let secret = accounts.get(name).ok_or(RegistrationError::InvalidConfig)?;
        use secrecy::ExposeSecret;
        let result = probe_registration(
            make_config(server, transport, name, secret.expose_secret())?,
            CancellationToken::new(),
        )
        .await?;
        if result.challenges < 1
            || result.refreshes != 1
            || !result.unregistered
            || !result.contact_verified
            || !result.call_id_stable
            || !result.cseq_increased
        {
            return Err(RegistrationError::Transport);
        }
        rows.push(
            serde_json::json!({"transport":result.transport,"challenges":result.challenges,
            "registration":200,"explicit_refresh":200,"unregistration":200,
            "contact_verified":result.contact_verified,"call_id_stable":result.call_id_stable,
            "cseq_increased":result.cseq_increased}),
        );
        let wrong_transport = match label {
            "udp" => Transport::Udp,
            "tcp" => Transport::Tcp,
            _ => Transport::Tls {
                ca_pem: ca.clone(),
                server_name: "pbx".into(),
            },
        };
        if !matches!(
            probe_registration(
                make_config(
                    server,
                    wrong_transport,
                    name,
                    "deliberately-wrong-fictional-password"
                )?,
                CancellationToken::new()
            )
            .await,
            Err(RegistrationError::Authentication)
        ) {
            return Err(RegistrationError::Transport);
        }
    }
    for (trust, identity) in [(untrusted, "pbx"), (ca, "wrong.lab.invalid")] {
        let config = make_config(
            SocketAddr::new(address.ip(), 5061),
            Transport::Tls {
                ca_pem: trust,
                server_name: identity.into(),
            },
            "1201",
            "unused-fictional-password",
        )?;
        if !matches!(
            probe_registration(config, CancellationToken::new()).await,
            Err(RegistrationError::Transport)
        ) {
            return Err(RegistrationError::Transport);
        }
    }
    println!(
        "{}",
        serde_json::json!({"schema_version":1,"fixture":"candidate-rust-registration",
        "rsipstack":"0.7.3","transports":rows,"wrong_password_rejected":true,
        "untrusted_ca_rejected":true,"wrong_hostname_rejected":true,"calls_tested":false,"g0_passed":false})
    );
    Ok(())
}
