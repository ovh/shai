pub struct AgentMeta {
    model: String,
    provider: String,
    name: Option<String>,
}

impl AgentMeta {
    pub fn new() -> Self {
        Self {
            model: String::new(),
            provider: String::new(),
            name: None,
        }
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn set_model(&mut self, model: String) {
        self.model = model;
    }

    pub fn provider(&self) -> &str {
        &self.provider
    }

    pub fn set_provider(&mut self, provider: String) {
        self.provider = provider;
    }

    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    pub fn set_name(&mut self, name: Option<String>) {
        self.name = name;
    }
}

impl Default for AgentMeta {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_starts_empty() {
        let meta = AgentMeta::new();
        assert_eq!(meta.model(), "");
        assert_eq!(meta.provider(), "");
        assert_eq!(meta.name(), None);
    }

    #[test]
    fn test_setters() {
        let mut meta = AgentMeta::new();
        meta.set_model("gpt-4".to_string());
        meta.set_provider("openai".to_string());
        meta.set_name(Some("custom-agent".to_string()));
        assert_eq!(meta.model(), "gpt-4");
        assert_eq!(meta.provider(), "openai");
        assert_eq!(meta.name(), Some("custom-agent"));
    }
}
