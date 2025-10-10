//! Utility for retrieving the maximum context length for known LLM models.

const MIN_SIMILARITY_THRESHOLD: f64 = 0.6;

/// Calculate similarity score between two strings using Jaro-Winkler distance
fn similarity_score(s1: &str, s2: &str) -> f64 {
    let s1_lower = s1.to_lowercase();
    let s2_lower = s2.to_lowercase();

    let s1_chars: Vec<char> = s1_lower.chars().collect();
    let s2_chars: Vec<char> = s2_lower.chars().collect();

    let max_len = s1_chars.len().max(s2_chars.len());
    if max_len == 0 {
        return 1.0;
    }

    let mut matches = 0;
    let min_len = s1_chars.len().min(s2_chars.len());

    for i in 0..min_len {
        if s1_chars[i] == s2_chars[i] {
            matches += 1;
        }
    }

    if s1_lower.contains(&s2_lower) || s2_lower.contains(&s1_lower) {
        matches += min_len / 2;
    }

    matches as f64 / max_len as f64
}

pub fn get_max_context(model_name: &str) -> usize {
    let models = [
        // OpenAI models
        ("gpt-oss", 131_000),

        // Mistral models
        ("mistral-small-3-2", 128_000),
        ("mistral-7b", 32_000),
        ("mistral-nemo", 32_000),
        ("mixtral-8x7b", 32_000),

        // Qwen models
        ("qwen3", 32_000),
        ("qwen-2-5", 32_000),

        // Llama models
        ("llama-3-1", 131_000),
        ("llama-3_3", 131_000),
        ("meta-llama-3_3", 131_000),
        ("meta-llama-3_1", 131_000),

        // Deepseek models
        ("deepseek-r1", 128_000),
    ];

    // Try exact match first
    for (model, context) in models.iter() {
        if *model == model_name {
            return *context;
        }
    }

    // Fuzzy matching with minimum threshold
    let mut best_match: Option<(f64, usize)> = None;

    for (model, context) in models.iter() {
        let score = similarity_score(model_name, model);
        if score >= MIN_SIMILARITY_THRESHOLD {
            if let Some((best_score, _)) = best_match {
                if score > best_score {
                    best_match = Some((score, *context));
                }
            } else {
                best_match = Some((score, *context));
            }
        }
    }

    best_match.map(|(_, context)| context).unwrap_or(30_096)
}
