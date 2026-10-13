use super::*;
use secrecy::ExposeSecret;
use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::sync::atomic::{AtomicUsize, Ordering};

const SECRET: &str = "s3cret-marker-9f2";

type Check = fn(&ConfigError) -> bool;

/// Unique temporary directory under `std::env::temp_dir()`, removed on drop.
struct TempDir(PathBuf);
impl TempDir {
    fn new() -> Self {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "rust-microsip-config-test-{}-{n}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap_or_else(|_| unreachable!("temp dir"));
        Self(path)
    }
    fn file(&self, name: &str, text: &str, mode: u32) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, text).unwrap_or_else(|_| unreachable!("write"));
        fs::set_permissions(&path, fs::Permissions::from_mode(mode))
            .unwrap_or_else(|_| unreachable!("chmod"));
        path
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn uid() -> u32 {
    files::current_uid().unwrap_or_else(|_| unreachable!("uid"))
}

fn ctx() -> ResolveContext {
    ResolveContext {
        uid: uid(),
        ..ResolveContext::default()
    }
}

fn env(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

const BASE_TOML: &str = "schema = 1\n[account]\nusername = \"1001\"\ndomain = \"pbx.invalid\"\n";

fn ok(ctx: &ResolveContext) -> EffectiveConfig {
    resolve(ctx).unwrap_or_else(|e| unreachable!("resolve failed: {e}"))
}
fn err(ctx: &ResolveContext) -> ConfigError {
    match resolve(ctx) {
        Ok(_) => unreachable!("expected an error"),
        Err(e) => e,
    }
}

#[test]
fn every_precedence_combination_picks_the_highest_source() {
    let dir = TempDir::new();
    for mask in 0..32u8 {
        let has = |bit: u8| mask & (1 << bit) != 0;
        let mut c = ctx();
        let mut toml = BASE_TOML.to_owned();
        if has(0) {
            toml.push_str("server = \"toml.invalid\"\n[ui]\ntheme = \"light\"\n");
        }
        c.config_path = Some(dir.file("c.toml", &toml, 0o600));
        if has(1) {
            let text = "RUST_MICROSIP_ACCOUNT_SERVER=dotenv.invalid\nRUST_MICROSIP_UI_THEME=dark\n";
            c.env_file = Some(dir.file("e.env", text, 0o600));
        }
        if has(2) {
            c.env = env(&[
                ("RUST_MICROSIP_ACCOUNT_SERVER", "env.invalid"),
                ("RUST_MICROSIP_UI_THEME", "system"),
            ]);
        }
        if has(3) {
            c.flags.insert(Field::Server, "flag.invalid".into());
            c.flags.insert(Field::Theme, "light".into());
        }
        if has(4) {
            c.sqlite = sqlite_layer(&env(&[
                ("account.server", "sqlite.invalid"),
                ("ui.theme", "dark"),
                ("not.a_key", "ignored"),
            ]));
        }
        let cfg = ok(&c);
        let (server, theme, source) = if has(3) {
            ("flag.invalid", Theme::Light, Source::Flag)
        } else if has(2) {
            ("env.invalid", Theme::System, Source::Env)
        } else if has(1) {
            ("dotenv.invalid", Theme::Dark, Source::DotEnv)
        } else if has(0) {
            ("toml.invalid", Theme::Light, Source::Toml)
        } else if has(4) {
            ("sqlite.invalid", Theme::Dark, Source::Sqlite)
        } else {
            ("pbx.invalid", Theme::System, Source::Default)
        };
        assert_eq!(cfg.config.account.server, server, "mask {mask}");
        assert_eq!(cfg.config.ui.theme, theme, "mask {mask}");
        assert_eq!(
            (cfg.source(Field::Server), cfg.source(Field::Theme)),
            (source, source),
            "mask {mask}"
        );
        assert_eq!(cfg.source(Field::Username), Source::Toml);
    }
}

#[test]
fn defaults_follow_transport_and_domain() {
    let mut c = ctx();
    c.env = env(&[
        ("RUST_MICROSIP_ACCOUNT_USERNAME", "1001"),
        ("RUST_MICROSIP_ACCOUNT_DOMAIN", "pbx.invalid"),
        ("RUST_MICROSIP_ACCOUNT_TRANSPORT", "tls"),
        ("RUST_MICROSIP_ACCOUNT_MEDIA", "srtp"),
        ("RUST_MICROSIP_UI_DND", "1"),
    ]);
    let cfg = ok(&c);
    let a = &cfg.config.account;
    assert_eq!(
        (a.port, a.server.as_str(), a.expires),
        (5061, "pbx.invalid", DEFAULT_EXPIRES)
    );
    assert_eq!(cfg.config.password_env, DEFAULT_PASSWORD_ENV);
    assert!(cfg.config.ui.dnd && cfg.password.is_none() && cfg.config_file.is_none());
    assert_eq!(cfg.source(Field::Port), Source::Default);
}

#[test]
fn missing_and_invalid_values_are_distinguished() {
    assert!(matches!(err(&ctx()), ConfigError::Missing(Field::Domain)));
    let mut c = ctx();
    c.env = env(&[("RUST_MICROSIP_ACCOUNT_DOMAIN", "pbx.invalid")]);
    assert!(matches!(err(&c), ConfigError::Missing(Field::Username)));

    let dir = TempDir::new();
    c.config_path = Some(dir.file("c.toml", &format!("{BASE_TOML}port = 5070\n"), 0o600));
    for (key, value) in [
        ("RUST_MICROSIP_ACCOUNT_PORT", "70000"),
        ("RUST_MICROSIP_ACCOUNT_TRANSPORT", "UDP"),
        ("RUST_MICROSIP_ACCOUNT_EXPIRES", "0"),
        ("RUST_MICROSIP_UI_DND", "maybe"),
        ("RUST_MICROSIP_ACCOUNT_CA_FILE", "relative.pem"),
        ("RUST_MICROSIP_ACCOUNT_PASSWORD_ENV", "1BAD"),
    ] {
        c.env = env(&[(key, value)]);
        // An invalid higher source is an error, never a silent fallback to TOML.
        assert!(
            matches!(
                err(&c),
                ConfigError::Invalid {
                    origin: Source::Env,
                    ..
                }
            ),
            "{key}"
        );
    }
    c.env = env(&[("RUST_MICROSIP_ACCOUNT_SERVER", "evil host")]);
    assert!(matches!(
        err(&c),
        ConfigError::Validation(ValidationError::InvalidCharacters)
    ));
    c.env = env(&[("RUST_MICROSIP_ACCOUNT_MEDIA", "srtp")]);
    assert!(matches!(
        err(&c),
        ConfigError::Validation(ValidationError::SrtpRequiresTls)
    ));
}

#[test]
fn toml_schema_types_and_unknown_keys_are_rejected() {
    let dir = TempDir::new();
    let cases: [(&str, Check); 7] = [
        ("[account]\nusername = \"1\"\n", |e| {
            matches!(e, ConfigError::UnsupportedSchema(_))
        }),
        ("schema = 2\n", |e| {
            matches!(e, ConfigError::UnsupportedSchema(_))
        }),
        (
            "schema = 1\nextra = 1\n",
            |e| matches!(e, ConfigError::UnknownKey { key, .. } if key == "extra"),
        ),
        ("schema = 1\n[telephony]\n", |e| {
            matches!(e, ConfigError::UnknownKey { .. })
        }),
        (
            "schema = 1\n[account]\nfoo = 1\n",
            |e| matches!(e, ConfigError::UnknownKey { key, .. } if key == "account.foo"),
        ),
        (
            "schema = 1\n[account]\nport = \"5060\"\n",
            |e| matches!(e, ConfigError::WrongType { key, .. } if key == "account.port"),
        ),
        ("schema = 1\n[account\n", |e| {
            matches!(e, ConfigError::TomlSyntax { line: 2, .. })
        }),
    ];
    for (text, check) in cases {
        let mut c = ctx();
        c.config_path = Some(dir.file("c.toml", text, 0o600));
        let e = err(&c);
        assert!(check(&e), "{text:?} -> {e:?}");
    }
}

#[test]
fn explicit_files_must_exist_but_xdg_default_is_optional() {
    let dir = TempDir::new();
    let mut c = ctx();
    c.config_path = Some(dir.0.join("absent.toml"));
    assert!(matches!(err(&c), ConfigError::FileMissing(_)));
    c.config_path = None;
    c.env_file = Some(dir.0.join("absent.env"));
    assert!(matches!(err(&c), ConfigError::FileMissing(_)));

    let base = [
        ("RUST_MICROSIP_ACCOUNT_USERNAME", "1"),
        ("RUST_MICROSIP_ACCOUNT_DOMAIN", "d.invalid"),
    ];
    let mut c = ctx();
    c.env = env(&base);
    c.env
        .insert("XDG_CONFIG_HOME".into(), dir.0.display().to_string());
    assert!(ok(&c).config_file.is_none());

    fs::create_dir_all(dir.0.join("rust-microsip")).unwrap_or_else(|_| unreachable!());
    let path = dir.file(
        "rust-microsip/config.toml",
        "schema = 1\n[ui]\ntheme = \"dark\"\n",
        0o644,
    );
    assert_eq!(ok(&c).config_file.as_ref(), Some(&path));
    // HOME fallback and relative XDG_CONFIG_HOME being ignored.
    c.env = env(&base);
    c.env.insert("XDG_CONFIG_HOME".into(), "relative".into());
    c.env.insert("HOME".into(), dir.0.display().to_string());
    assert_eq!(
        c.default_config_path(),
        Some(dir.0.join(".config/rust-microsip/config.toml"))
    );
    // An explicit --config replaces the XDG default instead of merging with it.
    c.env
        .insert("XDG_CONFIG_HOME".into(), dir.0.display().to_string());
    c.config_path = Some(dir.file("other.toml", "schema = 1\n", 0o600));
    assert_eq!(ok(&c).config.ui.theme, Theme::System);
}

#[test]
fn writable_or_secret_bearing_files_need_safe_permissions() {
    let dir = TempDir::new();
    let mut c = ctx();
    c.config_path = Some(dir.file("c.toml", BASE_TOML, 0o666));
    assert!(matches!(
        err(&c),
        ConfigError::InsecureFile {
            problem: FileProblem::Writable,
            ..
        }
    ));
    c.config_path = Some(dir.file("c.toml", BASE_TOML, 0o644));
    assert!(ok(&c).password.is_none());

    let literal = format!("{BASE_TOML}password = \"{SECRET}\"\n");
    c.config_path = Some(dir.file("c.toml", &literal, 0o640));
    let e = err(&c);
    assert!(matches!(
        e,
        ConfigError::InsecureFile {
            problem: FileProblem::NotPrivate,
            ..
        }
    ));
    assert!(!e.to_string().contains(SECRET) && e.to_string().contains("chmod 600"));
    c.config_path = Some(dir.file("c.toml", &literal, 0o600));
    let cfg = ok(&c);
    assert!(cfg.literal_password_warning);
    assert_eq!(cfg.password_source, Some(PasswordSource::TomlLiteral));
    c.uid = uid().wrapping_add(1);
    // A file owned by someone else is refused before its contents are used.
    assert!(matches!(
        err(&c),
        ConfigError::InsecureFile {
            problem: FileProblem::ForeignOwner,
            ..
        }
    ));
    let pem = dir.file("ca.pem", "-----BEGIN CERTIFICATE-----\n", 0o644);
    assert!(crate::read_ca_file(&pem, uid()).is_ok());
    assert!(matches!(
        crate::read_ca_file(&pem, uid().wrapping_add(1)),
        Err(ConfigError::InsecureFile {
            problem: FileProblem::ForeignOwner,
            ..
        })
    ));
    let writable = dir.file("ca-w.pem", "-----BEGIN CERTIFICATE-----\n", 0o666);
    assert!(matches!(
        crate::read_ca_file(&writable, uid()),
        Err(ConfigError::InsecureFile {
            problem: FileProblem::Writable,
            ..
        })
    ));

    let mut c = ctx();
    c.config_path = Some(dir.file("c.toml", BASE_TOML, 0o600));
    c.env_file = Some(dir.file("e.env", "RUST_MICROSIP_UI_THEME=dark\n", 0o644));
    assert!(ok(&c).password.is_none());
    c.env_file = Some(dir.file(
        "e.env",
        &format!("RUST_MICROSIP_SIP_PASSWORD={SECRET}\n"),
        0o644,
    ));
    assert!(matches!(
        err(&c),
        ConfigError::InsecureFile {
            problem: FileProblem::NotPrivate,
            ..
        }
    ));
}

#[test]
fn password_file_must_be_private_regular_and_not_a_symlink() {
    let dir = TempDir::new();
    let mut c = ctx();
    c.config_path = Some(dir.file("c.toml", BASE_TOML, 0o600));
    let pw = dir.file("pw", &format!("{SECRET}\r\n"), 0o600);
    c.flags
        .insert(Field::PasswordFile, pw.display().to_string());
    let cfg = ok(&c);
    assert_eq!(
        cfg.password.as_ref().map(|s| s.expose_secret()),
        Some(SECRET)
    );
    assert_eq!(cfg.password_source, Some(PasswordSource::File));

    fs::set_permissions(&pw, fs::Permissions::from_mode(0o604)).unwrap_or_else(|_| unreachable!());
    assert!(matches!(
        err(&c),
        ConfigError::InsecureFile {
            problem: FileProblem::NotPrivate,
            ..
        }
    ));
    fs::set_permissions(&pw, fs::Permissions::from_mode(0o600)).unwrap_or_else(|_| unreachable!());
    let link = dir.0.join("link");
    symlink(&pw, &link).unwrap_or_else(|_| unreachable!());
    c.flags
        .insert(Field::PasswordFile, link.display().to_string());
    assert!(matches!(
        err(&c),
        ConfigError::InsecureFile {
            problem: FileProblem::Symlink,
            ..
        }
    ));
    c.flags
        .insert(Field::PasswordFile, dir.0.display().to_string());
    assert!(matches!(
        err(&c),
        ConfigError::InsecureFile {
            problem: FileProblem::NotRegular,
            ..
        }
    ));
    c.flags.insert(
        Field::PasswordFile,
        dir.file("empty", "\n", 0o600).display().to_string(),
    );
    assert!(matches!(err(&c), ConfigError::EmptyPassword(_)));
}

#[test]
fn password_sources_follow_documented_order() {
    let dir = TempDir::new();
    let mut c = ctx();
    let toml = format!("{BASE_TOML}password_env = \"LAB_PW\"\npassword = \"literal\"\n");
    c.config_path = Some(dir.file("c.toml", &toml, 0o600));
    c.flags.insert(
        Field::PasswordFile,
        dir.file("pw", "file", 0o600).display().to_string(),
    );
    c.env_file = Some(dir.file("e.env", "LAB_PW=dotenv\n", 0o600));
    c.env = env(&[("LAB_PW", "env")]);
    let order = [
        ("env", PasswordSource::Env),
        ("dotenv", PasswordSource::DotEnv),
        ("file", PasswordSource::File),
        ("literal", PasswordSource::TomlLiteral),
    ];
    for (expected, source) in order {
        let cfg = ok(&c);
        assert_eq!(
            cfg.password.as_ref().map(|s| s.expose_secret()),
            Some(expected)
        );
        assert_eq!(cfg.password_source, Some(source));
        match source {
            PasswordSource::Env => c.env.clear(),
            PasswordSource::DotEnv => c.env_file = None,
            _ => {
                c.flags.clear();
            }
        }
    }
}

#[test]
fn secrets_never_appear_in_debug_errors_exports_or_doctor() {
    let dir = TempDir::new();
    let mut c = ctx();
    let toml = format!("{BASE_TOML}password = \"{SECRET}\"\n");
    c.config_path = Some(dir.file("c.toml", &toml, 0o600));
    let cfg = ok(&c);
    let report = DoctorReport::new(&Ok(cfg));
    let cfg = ok(&c);
    for text in [
        format!("{cfg:?}"),
        cfg.config.to_toml().unwrap_or_default(),
        report.to_json(),
        report.to_string(),
    ] {
        assert!(!text.contains(SECRET), "{text}");
    }
    assert!(report.to_json().contains("\"password_present\":true"));
    assert!(
        report
            .to_json()
            .contains("\"password_source\":\"toml_literal\"")
    );

    // Values never leak through error messages.
    c.env = env(&[("RUST_MICROSIP_ACCOUNT_PORT", SECRET)]);
    let e = err(&c);
    assert!(!e.to_string().contains(SECRET) && !format!("{e:?}").contains(SECRET));
    let failure = DoctorReport::new(&Err(e));
    assert!(
        !failure.ok
            && failure
                .to_json()
                .starts_with("{\"ok\":false,\"error\":\"invalid value")
    );
    c.env.clear();
    for bad in [
        format!("schema = 1\n[account]\nport = \"{SECRET}\"\n"),
        format!("schema = 1\nx = \"{SECRET}\n"),
    ] {
        c.config_path = Some(dir.file("c.toml", &bad, 0o600));
        let e = err(&c);
        assert!(
            !e.to_string().contains(SECRET) && !format!("{e:?}").contains(SECRET),
            "{e}"
        );
    }
}

#[test]
fn atomic_save_round_trips_without_secrets() {
    let dir = TempDir::new();
    let mut c = ctx();
    let toml = format!(
        "{BASE_TOML}display_name = \"Lab Désk\"\ntransport = \"tls\"\nmedia = \"srtp\"\n\
         ca_file = \"/etc/lab-ca.pem\"\npassword = \"{SECRET}\"\n[ui]\ndnd = true\n[audio]\ninput_device = \"USB\"\n"
    );
    c.config_path = Some(dir.file("c.toml", &toml, 0o600));
    let original = ok(&c).config;

    let target = dir.0.join("nested/deeper/config.toml");
    save(&original, &target).unwrap_or_else(|e| unreachable!("save: {e}"));
    let mode = |p: &Path| {
        fs::metadata(p)
            .map(|m| m.permissions().mode() & 0o777)
            .unwrap_or(0)
    };
    assert_eq!(mode(&target), 0o600);
    assert_eq!(mode(&dir.0.join("nested/deeper")), 0o700);
    let saved = fs::read_to_string(&target).unwrap_or_default();
    assert!(!saved.contains(SECRET) && !saved.contains("password ="));
    let leftovers = fs::read_dir(dir.0.join("nested/deeper"))
        .map(Iterator::count)
        .unwrap_or(0);
    assert_eq!(leftovers, 1, "temporary file left behind");

    // Overwrite in place, then reload.
    save(&original, &target).unwrap_or_else(|e| unreachable!("save: {e}"));
    c.config_path = Some(target);
    let reloaded = ok(&c);
    assert_eq!(reloaded.config, original);
    assert!(!reloaded.literal_password_warning && reloaded.password.is_none());
    assert_eq!(reloaded.config.account.display_name, "Lab Désk");
}

#[test]
fn dotenv_parsing_edge_cases() {
    let text = "# comment\r\n\
                \r\n\
                export A=plain value  # trailing\r\n\
                B='single # kept'\n\
                C=\"dq \\\"x\\\" \\\\ \\n\" # c\n\
                D=a#b\n\
                E=\n\
                F = spaced\n\
                A=last\n";
    let map = parse_dotenv(text).unwrap_or_else(|e| unreachable!("{e}"));
    let get = |k: &str| map.get(k).map(String::as_str);
    assert_eq!(get("A"), Some("last"));
    assert_eq!(get("B"), Some("single # kept"));
    assert_eq!(get("C"), Some("dq \"x\" \\ \n"));
    assert_eq!(get("D"), Some("a#b"));
    assert_eq!(get("E"), Some(""));
    assert_eq!(get("F"), Some("spaced"));
    for (bad, line) in [
        ("A=1\nnoequals\n", 2),
        ("1A=x", 1),
        ("A-B=x", 1),
        ("A=\"open", 1),
        ("A='open", 1),
        ("A=\"x\" junk", 1),
        ("A=\"\\q\"", 1),
    ] {
        assert_eq!(parse_dotenv(bad), Err(DotEnvError { line }), "{bad:?}");
    }

    let dir = TempDir::new();
    let mut c = ctx();
    c.config_path = Some(dir.file("c.toml", BASE_TOML, 0o600));
    c.env_file = Some(dir.file("e.env", &"#".repeat(64 * 1024 + 1), 0o600));
    assert!(matches!(err(&c), ConfigError::FileTooLarge(_)));
    c.env_file = Some(dir.file("e.env", &format!("X={SECRET}\n\"bad"), 0o600));
    let e = err(&c);
    assert!(
        matches!(e, ConfigError::DotEnvSyntax { line: 2, .. }) && !e.to_string().contains(SECRET)
    );
}

#[test]
fn cli_arguments() {
    let parse = |args: &[&str]| parse_args(args.iter().map(OsString::from));
    assert_eq!(parse(&[]), Ok(CliArgs::default()));
    assert_eq!(
        parse(&[
            "--config",
            "/x/password.toml",
            "--env-file=/y.env",
            "--doctor",
            "--json"
        ]),
        Ok(CliArgs {
            command: CliCommand::Doctor { json: true },
            config: Some("/x/password.toml".into()),
            env_file: Some("/y.env".into()),
            uri: None,
            import: None,
        })
    );
    // One sip:/tel: link in run mode; never together with another command.
    assert_eq!(
        parse(&["sip:1002@pbx.invalid"]).map(|a| a.uri),
        Ok(Some("sip:1002@pbx.invalid".into()))
    );
    assert_eq!(
        parse(&["TEL:+5511"]).map(|a| a.command),
        Ok(CliCommand::Run)
    );
    assert_eq!(parse(&["sip:1", "sip:2"]), Err(CliError::Duplicate("URI")));
    assert_eq!(parse(&["--doctor", "sip:1"]), Err(CliError::Conflict));
    assert_eq!(parse(&["sipx:1"]), Err(CliError::UnknownArgument));
    assert_eq!(
        parse(&["config", "validate"]).map(|a| a.command),
        Ok(CliCommand::ConfigValidate)
    );
    assert_eq!(
        parse(&["config", "export"]).map(|a| a.command),
        Ok(CliCommand::ConfigExport)
    );
    assert_eq!(parse(&["-h"]).map(|a| a.command), Ok(CliCommand::Help));
    assert_eq!(
        parse(&["--version"]).map(|a| a.command),
        Ok(CliCommand::Version)
    );
    assert_eq!(
        parse(&["--password", SECRET]),
        Err(CliError::PasswordNotAccepted)
    );
    assert_eq!(
        parse(&[&format!("--sip-pass={SECRET}")]),
        Err(CliError::PasswordNotAccepted)
    );
    assert_eq!(
        parse(&["--config"]),
        Err(CliError::MissingValue("--config"))
    );
    assert_eq!(
        parse(&["--config=a", "--config=b"]),
        Err(CliError::Duplicate("--config"))
    );
    assert_eq!(parse(&["--json"]), Err(CliError::Conflict));
    assert_eq!(parse(&["--help", "--doctor"]), Err(CliError::Conflict));
    assert_eq!(
        parse(&["config", "import"]),
        Err(CliError::MissingValue("config import"))
    );
    let import = parse(&["--apply", "config", "import", "/x/a.toml"]);
    assert_eq!(
        import.as_ref().map(|a| (a.command, a.import.clone())),
        Ok((
            CliCommand::ConfigImport { apply: true },
            Some("/x/a.toml".into())
        ))
    );
    assert_eq!(
        parse(&["config", "import", "/a.toml"]).map(|a| a.command),
        Ok(CliCommand::ConfigImport { apply: false })
    );
    assert_eq!(parse(&["--doctor", "--apply"]), Err(CliError::Conflict));
    assert_eq!(
        parse(&["config", "import", "/a", "--apply", "--apply"]),
        Err(CliError::Duplicate("--apply"))
    );
    assert_eq!(parse(&["--doctor=1"]), Err(CliError::UnknownArgument));
    // Error variants carry no argument text, so nothing can be echoed.
    assert_eq!(parse(&[SECRET]), Err(CliError::UnknownArgument));
}

fn stored(pairs: &[(&str, &str)]) -> BTreeMap<Field, String> {
    sqlite_layer(&env(pairs))
}

#[test]
fn sqlite_layer_alone_resolves_and_invalid_values_name_the_origin() {
    let mut c = ctx();
    c.sqlite = stored(&[
        ("account.username", "1001"),
        ("account.domain", "pbx.invalid"),
        ("account.port", "5070"),
        ("ui.dnd", "true"),
    ]);
    let cfg = ok(&c);
    assert_eq!(cfg.config.account.port, 5070);
    assert!(cfg.config.ui.dnd);
    assert_eq!(cfg.source(Field::Domain), Source::Sqlite);
    assert_eq!(cfg.source(Field::Expires), Source::Default);
    let report = DoctorReport::new(&Ok(cfg));
    assert!(
        report
            .to_json()
            .contains(r#""key":"account.domain","value":"pbx.invalid","source":"sqlite""#)
    );
    assert!(report.to_string().contains("account.port = 5070 [sqlite]"));

    for (key, value, field) in [
        ("account.port", "70000x", Field::Port),
        ("account.transport", "carrier-pigeon", Field::Transport),
        ("ui.theme", "neon", Field::Theme),
        (
            "account.password_file",
            "relative/path",
            Field::PasswordFile,
        ),
    ] {
        c.sqlite.insert(field, value.into());
        let e = err(&c);
        assert!(
            matches!(e, ConfigError::Invalid { field: f, origin: Source::Sqlite } if f == field),
            "{key}: {e}"
        );
        let text = e.to_string();
        assert!(text.contains(key) && text.contains("sqlite") && !text.contains(value));
        c.sqlite.remove(&field);
    }
    // A valid higher source hides an invalid stored value.
    c.sqlite.insert(Field::Theme, "neon".into());
    c.flags.insert(Field::Theme, "dark".into());
    assert_eq!(ok(&c).source(Field::Theme), Source::Flag);
}

#[test]
fn edits_skip_unchanged_and_refuse_overridden_fields() {
    let mut c = ctx();
    c.sqlite = stored(&[
        ("account.username", "1001"),
        ("account.domain", "pbx.invalid"),
    ]);
    c.env = env(&[("RUST_MICROSIP_ACCOUNT_SERVER", "env.invalid")]);
    let cfg = ok(&c);
    let before = cfg.config.clone();

    // Unchanged: nothing to write, the env override is never written back.
    assert_eq!(
        sqlite_edits(cfg.provenance(), Some(&before), &before).ok(),
        Some(BTreeMap::new())
    );

    let mut after = before.clone();
    after.ui.theme = Theme::Dark;
    after.account.port = 5070;
    after.audio.input_device = Some("Mic".into());
    let edits = sqlite_edits(cfg.provenance(), Some(&before), &after)
        .unwrap_or_else(|e| unreachable!("{e}"));
    let expected: BTreeMap<String, Option<String>> = [
        ("account.port", Some("5070")),
        ("audio.input_device", Some("Mic")),
        ("ui.theme", Some("dark")),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v.map(str::to_owned)))
    .collect();
    assert_eq!(edits, expected);

    // Clearing an optional stored value yields `None`.
    let mut cleared = after.clone();
    cleared.audio.input_device = None;
    assert_eq!(
        sqlite_edits(cfg.provenance(), Some(&after), &cleared).ok(),
        Some(
            [("audio.input_device".to_owned(), None)]
                .into_iter()
                .collect()
        )
    );

    // Changing a field fixed by the environment is refused, naming the source.
    after.account.server = "ui.invalid".into();
    let e = sqlite_edits(cfg.provenance(), Some(&before), &after).err();
    assert!(matches!(
        e,
        Some(ConfigError::Overridden {
            field: Field::Server,
            origin: Source::Env
        })
    ));
    assert!(e.is_some_and(
        |e| e.to_string().contains("set by env") && !e.to_string().contains("ui.invalid")
    ));

    // Round trip: stored edits reappear after a "restart" (new resolution).
    after.account.server = before.account.server.clone();
    let edits = sqlite_edits(cfg.provenance(), Some(&before), &after)
        .unwrap_or_else(|e| unreachable!("{e}"));
    let mut db: BTreeMap<String, String> = env(&[
        ("account.username", "1001"),
        ("account.domain", "pbx.invalid"),
    ]);
    for (k, v) in edits {
        match v {
            Some(v) => db.insert(k, v),
            None => db.remove(&k),
        };
    }
    c.sqlite = sqlite_layer(&db);
    let restarted = ok(&c);
    assert_eq!(restarted.config.ui.theme, Theme::Dark);
    assert_eq!(restarted.config.account.port, 5070);
    assert_eq!(restarted.source(Field::Theme), Source::Sqlite);
    assert_eq!(restarted.source(Field::Server), Source::Env);
}

#[test]
fn first_run_edits_use_layer_sources_and_skip_overrides() {
    let mut c = ctx();
    c.env = env(&[("RUST_MICROSIP_ACCOUNT_SERVER", "env.invalid")]);
    assert!(matches!(err(&c), ConfigError::Missing(_)));
    let sources = layer_sources(&c).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(sources.get(&Field::Server), Some(&Source::Env));
    let mut c2 = ctx();
    c2.sqlite = stored(&[
        ("account.username", "1001"),
        ("account.domain", "pbx.invalid"),
    ]);
    let form = ok(&c2).config;
    let edits = sqlite_edits(&sources, None, &form).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(
        !edits.contains_key("account.server"),
        "env value never stored"
    );
    assert_eq!(
        edits.get("account.username"),
        Some(&Some("1001".to_owned()))
    );
    assert_eq!(edits.get("ui.theme"), Some(&Some("system".to_owned())));
    assert!(
        !edits.contains_key("account.auth_username"),
        "unset optional"
    );
    // Without a starting point nothing is ever removed: unset paths in the
    // form must not delete stored `ca_file`/`password_file` values.
    assert!(edits.values().all(Option::is_some), "{edits:?}");
    assert!(!edits.contains_key("account.ca_file") && !edits.contains_key("account.password_file"));
}

#[test]
fn import_preview_is_validated_sanitized_and_read_only() {
    let dir = TempDir::new();
    let text = format!(
        "schema = 1\n[account]\nusername = \"1001\"\ndomain = \"pbx.invalid\"\nport = 5070\npassword = \"{SECRET}\"\n[ui]\ndnd = true\n"
    );
    let path = dir.file("import.toml", &text, 0o600);
    let preview = preview_import(&path, uid()).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(preview.literal_password_ignored);
    let values = preview.sqlite_values();
    assert_eq!(
        values.keys().map(String::as_str).collect::<Vec<_>>(),
        [
            "account.domain",
            "account.port",
            "account.username",
            "ui.dnd"
        ]
    );
    assert_eq!(values.get("account.port").map(String::as_str), Some("5070"));
    let shown = preview.to_string();
    assert!(shown.contains("account.domain = pbx.invalid") && shown.contains("--apply"));
    assert!(!shown.contains(SECRET) && !format!("{preview:?}").contains(SECRET));
    assert!(values.values().all(|v| !v.contains(SECRET)));
    assert_eq!(
        fs::read_to_string(&path).ok(),
        Some(text.clone()),
        "file untouched"
    );

    // A literal password in a group-readable file is refused (CFG-06).
    let loose = dir.file("loose.toml", &text, 0o640);
    assert!(matches!(
        preview_import(&loose, uid()),
        Err(ConfigError::InsecureFile {
            problem: FileProblem::NotPrivate,
            ..
        })
    ));
    // Invalid or incomplete files fail before anything could be written.
    let bad = dir.file(
        "bad.toml",
        "schema = 1\n[account]\nusername = \"1001\"\ndomain = \"pbx.invalid\"\ntransport = \"x\"\n",
        0o600,
    );
    assert!(matches!(
        preview_import(&bad, uid()),
        Err(ConfigError::Invalid {
            field: Field::Transport,
            origin: Source::Toml
        })
    ));
    let partial = dir.file(
        "partial.toml",
        "schema = 1\n[ui]\ntheme = \"dark\"\n",
        0o600,
    );
    assert!(matches!(
        preview_import(&partial, uid()),
        Err(ConfigError::Missing(_))
    ));
    let unknown = dir.file("unknown.toml", "schema = 1\n[account]\nfoo = 1\n", 0o600);
    assert!(matches!(
        preview_import(&unknown, uid()),
        Err(ConfigError::UnknownKey { .. })
    ));
    assert!(matches!(
        preview_import(&dir.0.join("absent.toml"), uid()),
        Err(ConfigError::FileMissing(_))
    ));
}
