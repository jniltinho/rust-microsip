//! Layering rule: the domain stays free of infrastructure, adapters are
//! owned by one crate each, and the UI never reaches SIP, SQL, audio
//! backends, D-Bus or sockets directly. Dependencies come from
//! `cargo metadata` (offline), so renamed, table-form and target-specific
//! dependencies are seen by their real package names.

use serde_json::Value;
use std::path::{Path, PathBuf};

/// Allowed normal dependencies per workspace package; anything else fails.
const ALLOWED: &[(&str, &[&str])] = &[
    ("rust-microsip-core", &["thiserror", "secrecy"]),
    ("rust-microsip-media", &["rustrtc", "thiserror", "zeroize"]),
    (
        "rust-microsip-telephony",
        &[
            "rust-microsip-core",
            "rust-microsip-media",
            "base64",
            "ring",
            "rsipstack",
            "rustls",
            "tokio",
            "tokio-util",
            "thiserror",
            "secrecy",
            "zeroize",
        ],
    ),
    (
        "rust-microsip-audio",
        &["rust-microsip-core", "cpal", "thiserror"],
    ),
    (
        "rust-microsip-config",
        &[
            "rust-microsip-core",
            "serde",
            "toml",
            "secrecy",
            "thiserror",
        ],
    ),
    (
        "rust-microsip-storage",
        &["rust-microsip-core", "rusqlite", "thiserror"],
    ),
    (
        "rust-microsip-platform",
        &[
            "rust-microsip-core",
            "rust-microsip-control",
            "rust-microsip-platform-windows",
            "rustix",
            "serde_json",
            "zbus",
            "thiserror",
        ],
    ),
    (
        "rust-microsip-control",
        &["rust-microsip-core", "serde", "serde_json", "thiserror"],
    ),
    (
        "rust-microsip-platform-windows",
        &[
            "rust-microsip-core",
            "rust-microsip-control",
            "thiserror",
            "windows",
            "windows-sys",
        ],
    ),
    (
        "rust-microsip",
        &[
            "rust-microsip-core",
            "rust-microsip-telephony",
            "rust-microsip-audio",
            "rust-microsip-config",
            "rust-microsip-storage",
            "rust-microsip-platform",
            "rust-microsip-control",
            "ring",
            "serde_json",
            "rmcp",
            "eframe",
            "wgpu",
            "secrecy",
            "tokio",
        ],
    ),
];

/// Text the UI module must not contain: it talks to adapters only through
/// the core contract and the crates' public types.
const UI_FORBIDDEN: &[&str] = &[
    "rusqlite",
    "rsipstack",
    "rustls",
    "tokio",
    "cpal",
    "zbus",
    "net::",
    "UdpSocket",
    "TcpStream",
    "TcpListener",
    "rust_microsip_telephony",
    "rust_microsip_media",
];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn metadata() -> Result<Value, Box<dyn std::error::Error>> {
    let output = std::process::Command::new(env!("CARGO"))
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--offline",
            "--locked",
        ])
        .current_dir(root())
        .output()?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
    }
    Ok(serde_json::from_slice(&output.stdout)?)
}

/// Violations in `cargo metadata` output: workspace packages without a rule
/// and normal (non-dev, non-build) dependencies outside their allowlist.
fn violations(metadata: &Value, rules: &[(&str, &[&str])]) -> Vec<String> {
    let mut found = Vec::new();
    let packages = metadata["packages"]
        .as_array()
        .map_or(&[][..], Vec::as_slice);
    if packages.is_empty() {
        found.push("no workspace packages found".to_owned());
    }
    for package in packages {
        let name = package["name"].as_str().unwrap_or_default();
        let Some((_, allowed)) = rules.iter().find(|(krate, _)| *krate == name) else {
            found.push(format!("{name} has no layering rule"));
            continue;
        };
        for dependency in package["dependencies"].as_array().into_iter().flatten() {
            let target = dependency["name"].as_str().unwrap_or_default();
            if dependency["kind"].is_null() && !allowed.contains(&target) {
                found.push(format!("{name} depends on {target}"));
            }
        }
    }
    found
}

fn ui_violations(file: &str, source: &str) -> Vec<String> {
    source
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim_start().starts_with("//"))
        .flat_map(|(number, line)| {
            UI_FORBIDDEN
                .iter()
                .filter(move |name| line.contains(*name))
                .map(move |name| format!("{file}:{} names {name}", number + 1))
        })
        .collect()
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            rust_files(&path, out)?;
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
    Ok(())
}

#[test]
fn workspace_packages_only_use_their_allowed_dependencies() -> Result<(), Box<dyn std::error::Error>>
{
    let metadata = metadata()?;
    let members = metadata["packages"].as_array().map_or(0, Vec::len);
    assert_eq!(members, ALLOWED.len(), "every workspace package has a rule");
    let found = violations(&metadata, ALLOWED);
    assert!(found.is_empty(), "layering violations: {found:?}");
    Ok(())
}

#[test]
fn ui_module_does_not_reach_infrastructure() -> std::io::Result<()> {
    let mut files = Vec::new();
    rust_files(&root().join("apps/rust-microsip/src/ui"), &mut files)?;
    assert!(!files.is_empty());
    let mut found = Vec::new();
    for path in files {
        let name = path.display().to_string();
        found.extend(ui_violations(&name, &std::fs::read_to_string(&path)?));
    }
    assert!(found.is_empty(), "UI layering violations: {found:?}");
    Ok(())
}

#[test]
fn the_rules_reject_injected_violations() {
    // Shape of `cargo metadata`: renamed (`rename`), target-specific
    // (`target`) and dev dependencies are reported by real package name.
    let metadata = serde_json::json!({"packages": [
        {"name": "rust-microsip-core", "dependencies": [
            {"name": "thiserror", "kind": null},
            {"name": "sqlx", "kind": null, "rename": "db"},
            {"name": "zbus", "kind": null, "target": "cfg(unix)"},
            {"name": "tokio", "kind": "dev"}
        ]},
        {"name": "rust-microsip-extra", "dependencies": []}
    ]});
    assert_eq!(
        violations(&metadata, &[("rust-microsip-core", &["thiserror"])]),
        [
            "rust-microsip-core depends on sqlx",
            "rust-microsip-core depends on zbus",
            "rust-microsip-extra has no layering rule",
        ]
    );
    assert_eq!(
        violations(&serde_json::json!({}), ALLOWED),
        ["no workspace packages found"]
    );
    assert_eq!(
        ui_violations(
            "ui.rs",
            "// tokio is fine in a comment\nuse std::{net::UdpSocket};\n"
        ),
        ["ui.rs:2 names net::", "ui.rs:2 names UdpSocket"]
    );
}

/// The Windows adapter may use `unsafe`; it must keep every documented-unsafe
/// denial. Every other package inherits `unsafe_code = "forbid"`.
const UNSAFE_CRATE: &str = "crates/platform-windows/Cargo.toml";

/// Unsafe-policy problems in one manifest (design W2 of add-windows-port).
fn unsafe_policy(manifest: &str, text: &str) -> Vec<String> {
    let has = |line: &str| text.lines().any(|l| l.trim() == line);
    let mut out = Vec::new();
    if manifest == UNSAFE_CRATE {
        for required in [
            "unsafe_code = \"allow\"",
            "undocumented_unsafe_blocks = \"deny\"",
            "multiple_unsafe_ops_per_block = \"deny\"",
            "missing_docs = \"deny\"",
        ] {
            if !has(required) {
                out.push(format!("{manifest} lacks {required}"));
            }
        }
    } else if !(has("[lints]") && has("workspace = true")) || text.contains("unsafe_code") {
        out.push(format!(
            "{manifest} must use the workspace lints (unsafe forbidden)"
        ));
    }
    out
}

#[test]
fn unsafe_is_confined_to_the_windows_adapter() -> std::io::Result<()> {
    let root = root();
    let workspace = std::fs::read_to_string(root.join("Cargo.toml"))?;
    assert!(workspace.contains("unsafe_code = \"forbid\""));
    let mut found = Vec::new();
    let mut manifests = vec!["apps/rust-microsip/Cargo.toml".to_owned()];
    for entry in std::fs::read_dir(root.join("crates"))? {
        manifests.push(format!(
            "crates/{}/Cargo.toml",
            entry?.file_name().to_string_lossy()
        ));
    }
    for manifest in &manifests {
        let text = std::fs::read_to_string(root.join(manifest))?;
        found.extend(unsafe_policy(manifest, &text));
    }
    assert!(found.is_empty(), "unsafe policy: {found:?}");
    // Negative controls: a relaxed crate and an adapter missing a denial.
    assert!(
        !unsafe_policy(
            "crates/x/Cargo.toml",
            "[lints.rust]\nunsafe_code = \"allow\"\n"
        )
        .is_empty()
    );
    assert!(
        !unsafe_policy(
            "crates/x/Cargo.toml",
            "[lints]\nworkspace = true\n[lints.rust]\nunsafe_code = \"allow\"\n"
        )
        .is_empty()
    );
    assert!(
        !unsafe_policy(
            UNSAFE_CRATE,
            "unsafe_code = \"allow\"\nmissing_docs = \"deny\"\n"
        )
        .is_empty()
    );
    Ok(())
}
