/// Strip ANSI escape sequences from a string.
///
/// Handles CSI (`\x1b[...m`), clear-line (`\x1b[K`), and cursor-movement
/// sequences that commonly appear in tool output. Operates on `char`s so
/// multi-byte UTF-8 content (→, ●, …) is preserved — CSI syntax is pure
/// ASCII and can never collide with UTF-8 continuation bytes.
pub fn strip_ansi(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            if matches!(chars.peek(), Some('[')) {
                chars.next();
                for c in chars.by_ref() {
                    // CSI sequences end at a byte in 0x40..=0x7E ('@'..='~')
                    if ('\u{40}'..='\u{7e}').contains(&c) {
                        break;
                    }
                }
            } else {
                // ESC followed by single char (e.g. ESC c)
                chars.next();
            }
        } else {
            result.push(ch);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_ansi_color() {
        assert_eq!(strip_ansi("\x1b[31mhello\x1b[0m"), "hello",);
    }

    #[test]
    fn test_strip_ansi_no_codes() {
        assert_eq!(strip_ansi("hello world"), "hello world");
    }

    #[test]
    fn test_strip_ansi_mixed() {
        assert_eq!(
            strip_ansi("\x1b[1;32mOK\x1b[0m: \x1b[33mstuff\x1b[0m"),
            "OK: stuff",
        );
    }

    #[test]
    fn test_strip_ansi_clear_line() {
        assert_eq!(strip_ansi("abc\x1b[Kdef"), "abcdef");
    }

    #[test]
    fn test_strip_ansi_preserves_multibyte_utf8() {
        assert_eq!(strip_ansi("héllo → wörld"), "héllo → wörld");
        assert_eq!(
            strip_ansi("\x1b[36m→\x1b[0m \x1b[1m●\x1b[0m ◆ ✻ ✅"),
            "→ ● ◆ ✻ ✅"
        );
        assert_eq!(
            strip_ansi("\x1b[2m░ model on provider\x1b[0m"),
            "░ model on provider"
        );
    }
}
