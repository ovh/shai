pub mod clifixer;
pub mod coder;
pub mod compacter;
pub mod gerund;
pub mod searcher;

#[cfg(test)]
pub(crate) mod test_helpers {
    use std::sync::Once;

    use shai_llm::client::LlmClient;

    /// Mutex to serialize tests that call `std::env::set_current_dir`.
    /// Since the working directory is process-wide, parallel tests would
    /// otherwise interfere with each other.
    pub static DIR_TEST_MUTEX: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    static INIT_LOGGING: Once = Once::new();

    /// Initialize logging for tests. Safe to call multiple times.
    pub fn init_test_logging() {
        INIT_LOGGING.call_once(|| {
            let _ = crate::logging::LoggingConfig::from_env().init();
        });
    }

    /// Helper to get an LLM client + model from ShaiConfig.
    /// Falls back to environment variables if no config file exists.
    pub async fn get_llm() -> Result<(std::sync::Arc<LlmClient>, String), Box<dyn std::error::Error>>
    {
        let (client, model) = crate::config::config::ShaiConfig::get_llm().await?;
        Ok((std::sync::Arc::new(client), model))
    }

    /// Try to get an LLM client from available environment variables, fallback to Ollama
    pub fn get_test_llm_client() -> LlmClient {
        if let Some(client) = LlmClient::from_env_ovhcloud() {
            return client;
        }
        if let Some(client) = LlmClient::from_env_openai() {
            return client;
        }
        if let Some(client) = LlmClient::from_env_anthropic() {
            return client;
        }
        if let Some(client) = LlmClient::from_env_openrouter() {
            return client;
        }
        if let Some(client) = LlmClient::from_env_openai_compatible() {
            return client;
        }
        if let Some(client) = LlmClient::from_env_mistral() {
            return client;
        }

        // Fallback to Ollama (always returns Some)
        LlmClient::from_env_ollama().expect("Ollama should always be available as fallback")
    }

    /// Get the default model for a provider, with fallbacks
    pub async fn get_test_model_for_provider(client: &LlmClient) -> String {
        client.default_model().await.unwrap_or_else(|_| {
            match client.provider_name() {
                "openai" => "gpt-3.5-turbo".to_string(),
                "anthropic" => "claude-3-haiku-20240307".to_string(),
                "openrouter" => "openai/gpt-3.5-turbo".to_string(),
                "ovhcloud" => "gpt-3.5-turbo".to_string(),
                "mistral" => "mistral-tiny".to_string(),
                "ollama" => "llama2".to_string(),
                _ => "gpt-3.5-turbo".to_string(),
            }
        })
    }
}
