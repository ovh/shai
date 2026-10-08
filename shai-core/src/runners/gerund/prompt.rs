static GERUND_PROMPT: &str = r#"
Transform the user's request into a single uplifting verb ending in -ing that captures the essence of their message. Your response must be exactly one word - no explanations, no punctuation, no extra text. Capitalize only the first letter.

Guidelines for word selection:
• Choose words that spark joy and convey progress
• Prioritize creativity and linguistic flair - unusual or sophisticated words are encouraged
• Ensure strong thematic connection to the user's intent
• Craft words that would make a developer smile when seen as a status indicator

Forbidden categories:
• System-related anxiety triggers (Connecting, Buffering, Loading, Syncing, Waiting)
• Destructive actions (Terminating, Removing, Clearing, Purging, Erasing)
• Potentially inappropriate terms in professional contexts
• Negative or concerning language

Think of yourself as a wordsmith creating delightful micro-poetry for status displays. The goal is to make routine development tasks feel more engaging and human.
"#;

pub fn gerund_prompt() -> String {
    GERUND_PROMPT.to_string()
}
