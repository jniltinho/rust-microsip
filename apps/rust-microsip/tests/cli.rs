//! Observable scaffold contracts without network, audio, or personal data.

use std::process::{Command, Output};

fn run(args: &[&str]) -> std::io::Result<Output> {
    Command::new(env!("CARGO_BIN_EXE_rust-microsip"))
        .args(args)
        .env_clear()
        .output()
}

#[test]
fn help_is_available_without_configuration() -> std::io::Result<()> {
    let output = run(&["--help"])?;
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("G0 pending"));
    Ok(())
}

#[test]
fn version_identifies_a_development_build() -> std::io::Result<()> {
    let output = run(&["--version"])?;
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        concat!("rust-microsip ", env!("CARGO_PKG_VERSION"), "\n")
    );
    Ok(())
}

#[test]
fn doctor_json_is_explicit_about_unimplemented_features() -> std::io::Result<()> {
    let output = run(&["--doctor", "--json"])?;
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    for expected in [
        "\"schema_version\":1",
        "\"stage\":\"scaffold\"",
        "\"g0_passed\":false",
        "\"sip\":\"not_implemented\"",
        "\"media\":\"not_implemented\"",
        "\"desktop\":\"not_implemented\"",
        "\"storage\":\"not_implemented\"",
        "\"ipc\":\"not_implemented\"",
        "\"host_probed\":false",
    ] {
        assert!(stdout.contains(expected), "missing field: {expected}");
    }
    assert!(output.stderr.is_empty());
    Ok(())
}

#[test]
fn doctor_text_does_not_claim_to_probe_the_host() -> std::io::Result<()> {
    let output = run(&["--doctor"])?;
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("does not probe"));
    Ok(())
}

#[test]
fn startup_without_a_desktop_fails_honestly() -> std::io::Result<()> {
    let output = run(&[])?;
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("not implemented yet"));
    Ok(())
}

#[test]
fn unsupported_or_conflicting_arguments_are_usage_errors() -> std::io::Result<()> {
    for args in [
        vec!["--json"],
        vec!["--doctor", "--doctor"],
        vec!["--help", "--version"],
        vec!["--version", "--json"],
        vec!["service"],
        vec!["mcp", "--stdio"],
        vec!["cli", "dial", "1001"],
    ] {
        let output = run(&args)?;
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }
    Ok(())
}

#[test]
fn invalid_arguments_do_not_echo_potential_secrets() -> std::io::Result<()> {
    let marker = "synthetic-password-marker-must-not-leak";
    let output = run(&["--password", marker])?;
    assert_eq!(output.status.code(), Some(2));
    assert!(!String::from_utf8_lossy(&output.stdout).contains(marker));
    assert!(!String::from_utf8_lossy(&output.stderr).contains(marker));
    Ok(())
}

#[test]
fn doctor_ignores_private_paths_and_environment() -> std::io::Result<()> {
    let output = Command::new(env!("CARGO_BIN_EXE_rust-microsip"))
        .args(["--doctor", "--json"])
        .env_clear()
        .env("HOME", "/nonexistent-private-profile")
        .env("XDG_CONFIG_HOME", "/nonexistent-private-config")
        .env("RUST_MICROSIP_SIP_PASSWORD", "synthetic-env-marker")
        .output()?;
    assert!(output.status.success());
    assert_eq!(output.stdout, run(&["--doctor", "--json"])?.stdout);
    assert!(!String::from_utf8_lossy(&output.stdout).contains("synthetic-env-marker"));
    Ok(())
}

#[test]
fn aliases_and_doctor_flag_order_are_supported() -> std::io::Result<()> {
    for (args, canonical) in [
        (vec!["-h"], vec!["--help"]),
        (vec!["-V"], vec!["--version"]),
        (vec!["--json", "--doctor"], vec!["--doctor", "--json"]),
    ] {
        assert_eq!(run(&args)?.stdout, run(&canonical)?.stdout);
        assert!(run(&args)?.status.success());
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn non_utf8_arguments_are_rejected_without_panicking() -> std::io::Result<()> {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let output = Command::new(env!("CARGO_BIN_EXE_rust-microsip"))
        .arg(OsString::from_vec(vec![0xff, 0xfe]))
        .env_clear()
        .output()?;
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    Ok(())
}
