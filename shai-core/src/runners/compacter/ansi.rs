/// Strip ANSI escape sequences from a string.
///
/// Handles CSI (`\x1b[...m`), clear-line (`\x1b[K`), and cursor-movement
/// sequences that commonly appear in tool output.
pub fn strip_ansi(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == 0x1b {
            // Skip escape sequence: ESC [ params final_byte
            i += 1;
            if i < bytes.len() && bytes[i] == b'[' {
                i += 1;
                while i < bytes.len() {
                    let c = bytes[i];
                    // CSI sequences end at byte 0x40..=0x7E
                    if (0x40..=0x7e).contains(&c) {
                        i += 1;
                        break;
                    }
                    i += 1;
                }
            } else {
                // ESC followed by single char (e.g. ESC c)
                i += 1;
            }
        } else {
            result.push(bytes[i] as char);
            i += 1;
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
}
