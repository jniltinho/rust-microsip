//! Compiled scaffold diagnostics, independent of configuration and I/O.
//!
//! This is neither the final application layer nor a telephony adapter.

/// Return static JSON v1 diagnostics without probing the host or accessing data.
///
/// Unavailable capabilities and pending G0 are explicit. Output is
/// valid UTF-8 with a trailing newline and contains no user-supplied values.
/// Do not interpret it as a call result or hardware test.
///
/// # Examples
///
/// ```
/// let report = rust_microsip::scaffold_diagnostic_json();
/// assert!(report.contains("\"stage\":\"scaffold\""));
/// assert!(report.contains("\"g0_passed\":false"));
/// ```
pub fn scaffold_diagnostic_json() -> &'static str {
    concat!(
        "{\"schema_version\":1,\"version\":\"",
        env!("CARGO_PKG_VERSION"),
        "\",\"stage\":\"scaffold\",\"g0_passed\":false,",
        "\"capabilities\":{\"sip\":\"not_implemented\",",
        "\"media\":\"not_implemented\",\"desktop\":\"not_implemented\",",
        "\"storage\":\"not_implemented\",\"ipc\":\"not_implemented\"},",
        "\"host_probed\":false}\n"
    )
}
