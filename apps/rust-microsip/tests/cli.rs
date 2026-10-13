//! Process-level CLI contracts with fictional data, no network or devices.

use std::process::{Command, Output};

const MARKER: &str = "fictional-secret-marker-7731";

fn run(args: &[&str], env: &[(&str, &str)]) -> std::io::Result<Output> {
    Command::new(env!("CARGO_BIN_EXE_rust-microsip"))
        .args(args)
        .env_clear()
        .envs(env.iter().copied())
        .output()
}
fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}
fn temp_dir(name: &str) -> std::io::Result<std::path::PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("rust-microsip-cli-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
    Ok(dir)
}
fn write_config(dir: &std::path::Path) -> std::io::Result<std::path::PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join("config.toml");
    std::fs::write(
        &path,
        "schema = 1\n[account]\nusername = \"1001\"\ndomain = \"pbx.invalid\"\nserver = \"pbx.invalid\"\ntransport = \"udp\"\nmedia = \"rtp\"\n",
    )?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    Ok(path)
}

#[test]
fn help_and_version_need_no_configuration() -> std::io::Result<()> {
    let help = run(&["--help"], &[])?;
    assert!(help.status.success());
    let help = text(&help);
    assert!(help.contains("Usage: rust-microsip") && help.contains("never accepted as arguments"));
    let version = run(&["--version"], &[])?;
    assert!(version.status.success());
    assert_eq!(
        String::from_utf8_lossy(&version.stdout),
        concat!("rust-microsip ", env!("CARGO_PKG_VERSION"), "\n")
    );
    assert!(run(&["-V"], &[])?.status.success() && run(&["-h"], &[])?.status.success());
    Ok(())
}

#[test]
fn doctor_reports_without_revealing_the_password() -> std::io::Result<()> {
    let dir = temp_dir("doctor")?;
    let config = write_config(&dir)?;
    let config = config.to_string_lossy();
    for json in [false, true] {
        let mut args = vec!["--config", config.as_ref(), "--doctor"];
        if json {
            args.push("--json");
        }
        let output = run(&args, &[("RUST_MICROSIP_SIP_PASSWORD", MARKER)])?;
        assert!(output.status.success());
        let out = text(&output);
        assert!(!out.contains(MARKER));
        assert!(out.contains("1001"));
        if json {
            assert!(out.trim_start().starts_with('{') && out.contains("password_present"));
        }
    }
    std::fs::remove_dir_all(dir)
}

#[test]
fn validate_and_export_use_the_effective_configuration() -> std::io::Result<()> {
    let dir = temp_dir("export")?;
    let config = write_config(&dir)?;
    let config = config.to_string_lossy();
    let valid = run(&["--config", config.as_ref(), "config", "validate"], &[])?;
    assert!(valid.status.success(), "{}", text(&valid));
    let export = run(
        &["--config", config.as_ref(), "config", "export"],
        &[("RUST_MICROSIP_SIP_PASSWORD", MARKER)],
    )?;
    assert!(export.status.success());
    let out = text(&export);
    assert!(out.contains("username = \"1001\"") && !out.contains(MARKER));
    let missing = run(&["config", "validate"], &[])?;
    assert_eq!(missing.status.code(), Some(1));
    std::fs::remove_dir_all(dir)
}

#[test]
fn invalid_arguments_are_usage_errors_without_echo() -> std::io::Result<()> {
    for args in [
        vec!["--bogus"],
        vec![MARKER],
        vec!["--password", MARKER],
        vec![&format!("--password={MARKER}")],
        vec!["--doctor", "--json", "--json"],
    ] {
        let args: Vec<&str> = args.iter().map(|a| a.as_ref()).collect();
        let output = run(&args, &[])?;
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(!text(&output).contains(MARKER));
    }
    Ok(())
}

#[test]
fn non_utf8_arguments_are_rejected_without_panicking() -> std::io::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    let output = Command::new(env!("CARGO_BIN_EXE_rust-microsip"))
        .arg(std::ffi::OsStr::from_bytes(b"\xff\xfe"))
        .env_clear()
        .output()?;
    assert_eq!(output.status.code(), Some(2));
    Ok(())
}

#[test]
fn desktop_without_a_display_fails_honestly() -> std::io::Result<()> {
    let output = run(&[], &[])?;
    assert_eq!(output.status.code(), Some(1));
    assert!(text(&output).contains("cannot open the window"));
    Ok(())
}

#[test]
fn desktop_opens_the_history_store_without_changing_the_display_error() -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let dir = temp_dir("store")?;
    let data = dir.to_string_lossy().into_owned();
    let output = run(&[], &[("XDG_DATA_HOME", &data)])?;
    assert_eq!(output.status.code(), Some(1));
    assert!(text(&output).contains("cannot open the window"));
    let db = dir.join("rust-microsip").join("rust-microsip.sqlite3");
    assert_eq!(std::fs::metadata(&db)?.permissions().mode() & 0o777, 0o600);
    // An unreadable database is kept untouched; the session falls back.
    let junk = vec![b'x'; 4096];
    std::fs::write(&db, &junk)?;
    let output = run(&[], &[("XDG_DATA_HOME", &data)])?;
    assert_eq!(output.status.code(), Some(1));
    let out = text(&output);
    assert!(out.contains("cannot open the window") && out.contains("history storage unavailable"));
    assert_eq!(std::fs::read(&db)?, junk);
    std::fs::remove_dir_all(dir)
}
