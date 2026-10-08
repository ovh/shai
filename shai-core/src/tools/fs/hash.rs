/// Computes a short stable hash for a line of text.
///
/// Uses FNV-1a (Fowler-Noll-Vo) which requires no external dependencies
/// and provides good distribution for line-level identification.
/// Returns an 8-character hex string.
pub fn compute_line_hash(line: &str) -> String {
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut hash = FNV_OFFSET;
    for byte in line.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    format!("{:08x}", hash & 0xFFFFFFFF)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_consistent() {
        let h1 = compute_line_hash("hello world");
        let h2 = compute_line_hash("hello world");
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_hash_different_lines() {
        let h1 = compute_line_hash("hello world");
        let h2 = compute_line_hash("Hello World");
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_hash_empty_line() {
        let h = compute_line_hash("");
        assert_eq!(h.len(), 8);
    }

    #[test]
    fn test_hash_length() {
        let h = compute_line_hash("some content");
        assert_eq!(h.len(), 8);
    }
}
