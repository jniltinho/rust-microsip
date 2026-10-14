//! Caller text shown outside the window (notifications), on every platform.

const MAX_CALLER_CHARS: usize = 64;

/// Make caller text safe for a notification body: drop control and bidi
/// override characters, bound the length and escape the markup subset.
pub fn sanitize_caller(caller: &str) -> String {
    let mut out = String::new();
    let visible = caller.chars().filter(|&c| {
        !(c.is_control() || matches!(c, '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'))
    });
    for c in visible.take(MAX_CALLER_CHARS) {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    if out.trim().is_empty() {
        "Unknown caller".to_owned()
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caller_text_is_sanitized() {
        assert_eq!(sanitize_caller("Alice <1002>"), "Alice &lt;1002&gt;");
        assert_eq!(sanitize_caller("A\r\nB\u{0}\u{202e}C & D"), "ABC &amp; D");
        assert_eq!(sanitize_caller("\n\t "), "Unknown caller");
        assert_eq!(sanitize_caller(&"x".repeat(500)).len(), MAX_CALLER_CHARS);
        assert_eq!(
            sanitize_caller("<b>$(reboot)</b>"),
            "&lt;b&gt;$(reboot)&lt;/b&gt;"
        );
    }
}
