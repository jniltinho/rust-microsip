//! Windows only (add-windows-port W9): icon, version information and the
//! application manifest (per-monitor DPI v2, UTF-8, long paths, Windows 10+)
//! for both executables, compiled with mingw `windres` (no extra crate), and
//! the Linux-sized 8 MiB main-thread stack. Other targets: nothing to do.

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=windows/rust-microsip.manifest");
    println!("cargo:rerun-if-changed=assets/icons/rust-microsip-256.png");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    if let Err(e) = resources() {
        println!("cargo:warning=Windows resources not embedded: {e}");
    }
    // The Windows default main-thread stack is 1 MiB; Linux gives 8 MiB.
    println!("cargo:rustc-link-arg-bins=-Wl,--stack,8388608");
}

/// ICO holding one 256x256 PNG entry (supported since Windows Vista).
fn ico(png: &[u8]) -> Result<Vec<u8>, String> {
    let len = u32::try_from(png.len()).map_err(|e| e.to_string())?;
    let mut out = vec![0, 0, 1, 0, 1, 0];
    // Width/height 0 = 256, no palette, 1 plane, 32 bpp, size, data at offset 22.
    out.extend_from_slice(&[0, 0, 0, 0, 1, 0, 32, 0]);
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&22u32.to_le_bytes());
    out.extend_from_slice(png);
    Ok(out)
}

fn resources() -> Result<(), String> {
    let out = PathBuf::from(std::env::var("OUT_DIR").map_err(|e| e.to_string())?);
    let dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").map_err(|e| e.to_string())?);
    let png =
        std::fs::read(dir.join("assets/icons/rust-microsip-256.png")).map_err(|e| e.to_string())?;
    std::fs::write(out.join("rust-microsip.ico"), ico(&png)?).map_err(|e| e.to_string())?;
    let version = std::env::var("CARGO_PKG_VERSION").map_err(|e| e.to_string())?;
    let nums: Vec<&str> = version.split(['.', '-']).take(3).collect();
    let quad = format!("{},0", nums.join(","));
    let slash = |p: &Path| p.display().to_string().replace('\\', "/");
    let rc = format!(
        r#"1 ICON "{ico}"
1 24 "{manifest}"
1 VERSIONINFO
FILEVERSION {quad}
PRODUCTVERSION {quad}
FILEOS 0x40004
FILETYPE 0x1
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904b0"
    BEGIN
      VALUE "CompanyName", "rust-microsip"
      VALUE "FileDescription", "rust-microsip SIP softphone"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "rust-microsip"
      VALUE "LegalCopyright", "GPL-3.0-or-later"
      VALUE "ProductName", "rust-microsip"
      VALUE "ProductVersion", "{version}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#,
        ico = slash(&out.join("rust-microsip.ico")),
        manifest = slash(&dir.join("windows/rust-microsip.manifest")),
    );
    let rc_path = out.join("rust-microsip.rc");
    std::fs::write(&rc_path, rc).map_err(|e| e.to_string())?;
    let res = out.join("rust-microsip.res.o");
    let windres =
        std::env::var("WINDRES").unwrap_or_else(|_| "x86_64-w64-mingw32-windres".to_owned());
    let status = Command::new(&windres)
        .arg(&rc_path)
        .args(["-O", "coff", "-o"])
        .arg(&res)
        .status()
        .map_err(|e| format!("{windres}: {e}"))?;
    if !status.success() {
        return Err(format!("{windres} failed: {status}"));
    }
    println!("cargo:rustc-link-arg-bins={}", res.display());
    Ok(())
}
