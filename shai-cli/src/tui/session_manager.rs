pub struct SessionManager {
    session_id: String,
    last_assistant_response: String,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            session_id: uuid::Uuid::new_v4().to_string(),
            last_assistant_response: String::new(),
        }
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn set_session_id(&mut self, id: &str) {
        self.session_id = id.to_string();
    }

    pub fn last_assistant_response(&self) -> &str {
        &self.last_assistant_response
    }

    pub fn set_last_assistant_response(&mut self, response: &str) {
        self.last_assistant_response = response.to_string();
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_generates_uuid() {
        let sm = SessionManager::new();
        assert!(!sm.session_id().is_empty());
        assert_eq!(sm.last_assistant_response(), "");
    }

    #[test]
    fn test_set_session_id() {
        let mut sm = SessionManager::new();
        sm.set_session_id("abc-123");
        assert_eq!(sm.session_id(), "abc-123");
    }

    #[test]
    fn test_set_last_assistant_response() {
        let mut sm = SessionManager::new();
        sm.set_last_assistant_response("Hello world");
        assert_eq!(sm.last_assistant_response(), "Hello world");
    }
}
