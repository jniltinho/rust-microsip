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

#[test]
fn config_import_previews_then_applies_into_sqlite_only() -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let dir = temp_dir("import")?;
    let data = dir.join("data");
    let data_env = data.to_string_lossy().into_owned();
    let file = dir.join("import.toml");
    let content = format!(
        "schema = 1\n[account]\nusername = \"1009\"\ndomain = \"pbx.invalid\"\npassword = \"{MARKER}\"\n[ui]\ntheme = \"dark\"\n"
    );
    std::fs::write(&file, &content)?;
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600))?;
    let file_arg = file.to_string_lossy().into_owned();
    let env = [("XDG_DATA_HOME", data_env.as_str())];
    // Preview: sanitized, nothing written, no database created.
    let preview = run(&["config", "import", &file_arg], &env)?;
    assert!(preview.status.success(), "{}", text(&preview));
    let out = text(&preview);
    assert!(out.contains("account.username = 1009") && out.contains("--apply"));
    assert!(out.contains("literal password is not imported") && !out.contains(MARKER));
    assert!(!data.exists());
    // Apply: stored in SQLite in one transaction, the file is untouched.
    let applied = run(&["config", "import", &file_arg, "--apply"], &env)?;
    assert!(applied.status.success(), "{}", text(&applied));
    let out = text(&applied);
    assert!(out.contains("Imported into the local database") && !out.contains(MARKER));
    assert_eq!(std::fs::read_to_string(&file)?, content);
    let db = data.join("rust-microsip").join("rust-microsip.sqlite3");
    assert_eq!(std::fs::metadata(&db)?.permissions().mode() & 0o777, 0o600);
    assert!(!String::from_utf8_lossy(&std::fs::read(&db)?).contains(MARKER));
    // The stored settings are now part of the effective configuration.
    let export = run(&["config", "export"], &env)?;
    assert!(export.status.success(), "{}", text(&export));
    let out = text(&export);
    assert!(out.contains("username = \"1009\"") && out.contains("theme = \"dark\""));
    assert!(!out.contains(MARKER));
    // Higher sources still win over the database.
    let export = run(
        &["config", "export"],
        &[
            ("XDG_DATA_HOME", data_env.as_str()),
            ("RUST_MICROSIP_UI_THEME", "light"),
        ],
    )?;
    assert!(text(&export).contains("theme = \"light\""));
    std::fs::remove_dir_all(dir)
}

#[test]
fn config_import_rejects_invalid_files_and_stray_apply() -> std::io::Result<()> {
    let dir = temp_dir("import-bad")?;
    let data = dir.join("data");
    let data_env = data.to_string_lossy().into_owned();
    let file = dir.join("bad.toml");
    std::fs::write(&file, "schema = 1\n[account]\nbogus = 1\n")?;
    let file_arg = file.to_string_lossy().into_owned();
    let env = [("XDG_DATA_HOME", data_env.as_str())];
    let bad = run(&["config", "import", &file_arg, "--apply"], &env)?;
    assert_eq!(bad.status.code(), Some(1));
    assert!(text(&bad).contains("unknown key") && !data.exists());
    let missing = run(
        &["config", "import", "/nonexistent/rust-microsip.toml"],
        &env,
    )?;
    assert_eq!(missing.status.code(), Some(1));
    assert_eq!(
        run(&["config", "validate", "--apply"], &env)?.status.code(),
        Some(2)
    );
    assert_eq!(run(&["config", "import"], &env)?.status.code(), Some(2));
    std::fs::remove_dir_all(dir)
}

#[test]
fn malformed_files_never_echo_secret_lines() -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let dir = temp_dir("leak")?;
    let data = dir.join("data").to_string_lossy().into_owned();
    // Unterminated strings: parser messages would quote these lines.
    let toml = dir.join("bad.toml");
    std::fs::write(
        &toml,
        format!("schema = 1\n[account]\nusername = \"1001\"\npassword = \"{MARKER}\n"),
    )?;
    let dotenv = dir.join("bad.env");
    std::fs::write(&dotenv, format!("RUST_MICROSIP_SIP_PASSWORD=\"{MARKER}\n"))?;
    for file in [&toml, &dotenv] {
        std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o600))?;
    }
    let (toml, dotenv) = (
        toml.to_string_lossy().into_owned(),
        dotenv.to_string_lossy().into_owned(),
    );
    let env = [("XDG_DATA_HOME", data.as_str())];
    for args in [
        vec!["--config", &toml, "config", "validate"],
        vec!["--config", &toml, "config", "export"],
        vec!["--config", &toml, "--doctor"],
        vec!["--config", &toml, "--doctor", "--json"],
        vec!["config", "import", &toml],
        vec!["config", "import", &toml, "--apply"],
        vec!["--env-file", &dotenv, "config", "validate"],
        vec!["--env-file", &dotenv, "--doctor", "--json"],
    ] {
        let output = run(&args, &env)?;
        let out = text(&output);
        assert!(!out.contains(MARKER), "{args:?}: {out}");
        assert!(out.contains("line"), "{args:?}: {out}");
    }
    std::fs::remove_dir_all(dir)
}

#[test]
fn control_commands_never_start_an_owner() -> std::io::Result<()> {
    let dir = temp_dir("control")?;
    let runtime = dir.to_string_lossy().into_owned();
    let output = run(&["status", "--json"], &[("XDG_RUNTIME_DIR", &runtime)])?;
    assert_eq!(output.status.code(), Some(4), "{}", text(&output));
    let body = String::from_utf8_lossy(&output.stdout);
    assert!(body.contains("\"engine_unavailable\""), "{body}");
    // Nothing was created: no lock, socket or database next to it.
    assert!(!dir.join("rust-microsip").join("instance.lock").exists());
    let output = run(&["call", "dial", "1002"], &[("XDG_RUNTIME_DIR", &runtime)])?;
    assert_eq!(output.status.code(), Some(4));
    assert!(text(&output).contains("idempotency key: cli-"));
    // There is no way to approve from the command line.
    for args in [
        &["approve", "op-1"][..],
        &["call", "dial", "1002", "--yes"],
        &["call", "dial", "1002", "--confirm"],
        &["grant", "call_dial"],
    ] {
        let output = run(args, &[("XDG_RUNTIME_DIR", &runtime)])?;
        assert_eq!(output.status.code(), Some(2), "{args:?}: {}", text(&output));
    }
    std::fs::remove_dir_all(dir)
}
