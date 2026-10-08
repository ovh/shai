pub struct TokenCounter {
    input: u32,
    output: u32,
    cached: u32,
}

impl TokenCounter {
    pub fn new() -> Self {
        Self {
            input: 0,
            output: 0,
            cached: 0,
        }
    }

    pub fn add(&mut self, input: u32, output: u32, cached: u32) {
        self.input += input;
        self.output += output;
        self.cached += cached;
    }

    pub fn input_tokens(&self) -> u32 {
        self.input
    }

    pub fn output_tokens(&self) -> u32 {
        self.output
    }

    pub fn cached_tokens(&self) -> u32 {
        self.cached
    }

    pub fn total(&self) -> u32 {
        self.input + self.output
    }
}

impl Default for TokenCounter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_starts_at_zero() {
        let tc = TokenCounter::new();
        assert_eq!(tc.input_tokens(), 0);
        assert_eq!(tc.output_tokens(), 0);
        assert_eq!(tc.cached_tokens(), 0);
        assert_eq!(tc.total(), 0);
    }

    #[test]
    fn test_add_accumulates() {
        let mut tc = TokenCounter::new();
        tc.add(100, 50, 25);
        assert_eq!(tc.input_tokens(), 100);
        assert_eq!(tc.output_tokens(), 50);
        assert_eq!(tc.cached_tokens(), 25);
        assert_eq!(tc.total(), 150);

        tc.add(10, 5, 0);
        assert_eq!(tc.input_tokens(), 110);
        assert_eq!(tc.output_tokens(), 55);
        assert_eq!(tc.cached_tokens(), 25);
        assert_eq!(tc.total(), 165);
    }

    #[test]
    fn test_total_excludes_cached() {
        let mut tc = TokenCounter::new();
        tc.add(100, 200, 50);
        assert_eq!(tc.total(), 300);
    }
}
