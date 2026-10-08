use super::searcher::SearcherBrain;
use crate::agent::Agent;
use openai_dive::v1::resources::chat::{ChatMessage, ChatMessageContent};
use std::sync::Arc;
use tempfile::TempDir;

use crate::runners::test_helpers::{get_llm, init_test_logging, DIR_TEST_MUTEX};

// Helper function to create a searcher agent with goal
async fn create_searcher_agent_with_goal(goal: &str) -> impl Agent {
    let (llm_client, model) = get_llm().await.expect("No LLM provider available");
    println!("using model: {:?}", model);

    crate::agent::AgentBuilder::with_brain(Box::new(SearcherBrain::new(llm_client, model)))
        .goal(goal)
        .tools(vec![
            Box::new(crate::tools::FetchTool::new()),
            Box::new(crate::tools::FindTool::new()),
            Box::new(crate::tools::LsTool::new()),
            Box::new(crate::tools::ReadTool::new(Arc::new(
                crate::tools::FsOperationLog::new(),
            ))),
            Box::new(crate::tools::TodoReadTool::new(Arc::new(
                crate::tools::TodoStorage::new(),
            ))),
            Box::new(crate::tools::TodoWriteTool::new(Arc::new(
                crate::tools::TodoStorage::new(),
            ))),
        ])
        .build()
}

#[tokio::test]
async fn test_searcher_find_struct_definition() {
    init_test_logging();
    let _guard = DIR_TEST_MUTEX.lock().await;

    // Create a temporary directory for this test
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let temp_path = temp_dir.path();

    // Change to the temporary directory
    std::env::set_current_dir(temp_path).expect("Failed to change directory");

    // Create a sample Rust project structure
    std::fs::create_dir_all(temp_path.join("src")).expect("Failed to create src directory");

    let user_struct_code = r#"use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: u64,
    pub name: String,
    pub email: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl User {
    pub fn new(id: u64, name: String, email: String) -> Self {
        Self {
            id,
            name,
            email,
            created_at: chrono::Utc::now(),
        }
    }
    
    pub fn is_valid_email(&self) -> bool {
        self.email.contains('@')
    }
    
    pub fn get_domain(&self) -> Option<String> {
        self.email.split('@').nth(1).map(|s| s.to_string())
    }
}"#;

    let main_code = r#"mod user;
use user::User;

fn main() {
    let user = User::new(1, "John Doe".to_string(), "john@example.com".to_string());
    println!("User: {:?}", user);
    println!("Valid email: {}", user.is_valid_email());
}"#;

    std::fs::write(temp_path.join("src/user.rs"), user_struct_code)
        .expect("Failed to write user.rs");
    std::fs::write(temp_path.join("src/main.rs"), main_code).expect("Failed to write main.rs");

    println!(
        "🧪 Test: Finding User struct definition in temp directory: {:?}",
        temp_path
    );

    // Create a searcher agent with goal to find User struct
    let mut agent = create_searcher_agent_with_goal(
        "Find where the User struct is defined in this rust codebase. Provide the file path and explain its structure, fields, and methods. Be specific about what you found."
    ).await;

    // Run the agent
    let result = agent.run().await;

    // Verify the agent completed successfully
    assert!(
        result.is_ok(),
        "Searcher agent should complete successfully"
    );
    let agent_result = result.unwrap();
    assert!(agent_result.success, "Agent should report success");

    println!(
        "🔍 Agent completed search with {} messages",
        agent_result.trace.len()
    );

    // Check that the agent found the User struct in its response
    let final_message = agent_result
        .trace
        .last()
        .expect("Should have final message");
    if let ChatMessage::Assistant {
        content: Some(content),
        ..
    } = final_message
    {
        let content_text = match content {
            ChatMessageContent::Text(text) => text,
            _ => panic!("Expected text content"),
        };

        println!("📄 Agent response:\n{}", content_text);

        // Verify the agent found the User struct and provided details
        assert!(
            content_text.to_lowercase().contains("user"),
            "Should mention User struct"
        );
        assert!(
            content_text.contains("src/user.rs") || content_text.contains("user.rs"),
            "Should identify the correct file location"
        );
        assert!(
            content_text.contains("id")
                || content_text.contains("name")
                || content_text.contains("email"),
            "Should mention some struct fields"
        );
        assert!(
            content_text.contains("new")
                || content_text.contains("is_valid_email")
                || content_text.contains("method"),
            "Should mention some methods"
        );
    } else {
        panic!("Expected final assistant message with content");
    }
}

#[tokio::test]
async fn test_searcher_analyze_auth_feature() {
    init_test_logging();
    let _guard = DIR_TEST_MUTEX.lock().await;

    // Create a temporary directory for this test
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let temp_path = temp_dir.path();

    // Change to the temporary directory
    std::env::set_current_dir(temp_path).expect("Failed to change directory");

    // Create a sample authentication feature
    std::fs::create_dir_all(temp_path.join("src/auth")).expect("Failed to create auth directory");

    let auth_service_code = r#"use crate::models::User;
use bcrypt::{hash, verify};

pub struct AuthService {
    secret_key: String,
}

impl AuthService {
    pub fn new(secret_key: String) -> Self {
        Self { secret_key }
    }
    
    pub fn hash_password(&self, password: &str) -> Result<String, AuthError> {
        hash(password, 12).map_err(|_| AuthError::HashingFailed)
    }
    
    pub fn verify_password(&self, password: &str, hash: &str) -> bool {
        verify(password, hash).unwrap_or(false)
    }
    
    pub fn authenticate_user(&self, email: &str, password: &str) -> Result<User, AuthError> {
        // Mock authentication logic
        if email == "admin@example.com" && password == "admin123" {
            Ok(User::new(1, "Admin".to_string(), email.to_string()))
        } else {
            Err(AuthError::InvalidCredentials)
        }
    }
    
    pub fn generate_token(&self, user_id: u64) -> String {
        format!("token_for_user_{}", user_id)
    }
}

#[derive(Debug)]
pub enum AuthError {
    HashingFailed,
    InvalidCredentials,
    TokenExpired,
    DatabaseError,
}"#;

    let auth_controller_code = r#"use crate::auth::AuthService;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct LoginResponse {
    pub success: bool,
    pub token: Option<String>,
    pub user_id: Option<u64>,
    pub message: String,
}

pub struct AuthController {
    auth_service: AuthService,
}

impl AuthController {
    pub fn new(auth_service: AuthService) -> Self {
        Self { auth_service }
    }
    
    pub async fn login(&self, request: LoginRequest) -> LoginResponse {
        match self.auth_service.authenticate_user(&request.email, &request.password) {
            Ok(user) => {
                let token = self.auth_service.generate_token(user.id);
                LoginResponse {
                    success: true,
                    token: Some(token),
                    user_id: Some(user.id),
                    message: "Login successful".to_string(),
                }
            },
            Err(_) => LoginResponse {
                success: false,
                token: None,
                user_id: None,
                message: "Invalid credentials".to_string(),
            },
        }
    }
    
    pub async fn logout(&self, token: &str) -> bool {
        // Mock logout logic
        !token.is_empty()
    }
}"#;

    std::fs::write(temp_path.join("src/auth/service.rs"), auth_service_code)
        .expect("Failed to write auth service");
    std::fs::write(
        temp_path.join("src/auth/controller.rs"),
        auth_controller_code,
    )
    .expect("Failed to write auth controller");
    std::fs::write(
        temp_path.join("src/auth/mod.rs"),
        "pub mod service;\npub mod controller;",
    )
    .expect("Failed to write auth mod.rs");

    println!(
        "🧪 Test: Analyzing authentication feature in temp directory: {:?}",
        temp_path
    );

    // Create a searcher agent with goal to analyze auth feature
    let mut agent = create_searcher_agent_with_goal(
        "Analyze the authentication feature in this codebase. Explain how authentication works, what components are involved, and provide a summary of the authentication flow. Be specific about the structs, methods, and error handling you find."
    ).await;

    // Run the agent
    let result = agent.run().await;

    // Verify the agent completed successfully
    assert!(
        result.is_ok(),
        "Searcher agent should complete successfully"
    );
    let agent_result = result.unwrap();
    assert!(agent_result.success, "Agent should report success");

    println!(
        "🔍 Agent completed analysis with {} messages",
        agent_result.trace.len()
    );

    // Check that the agent analyzed the authentication feature
    let final_message = agent_result
        .trace
        .last()
        .expect("Should have final message");
    if let ChatMessage::Assistant {
        content: Some(content),
        ..
    } = final_message
    {
        let content_text = match content {
            ChatMessageContent::Text(text) => text,
            _ => panic!("Expected text content"),
        };

        println!("📄 Agent analysis:\n{}", content_text);

        // Verify the agent analyzed the auth feature properly
        assert!(
            content_text.to_lowercase().contains("auth"),
            "Should mention authentication"
        );
        assert!(
            content_text.to_lowercase().contains("password")
                || content_text.to_lowercase().contains("login"),
            "Should mention password or login functionality"
        );
        assert!(
            content_text.contains("AuthService")
                || content_text.contains("AuthController")
                || content_text.contains("service")
                || content_text.contains("controller"),
            "Should identify key authentication components"
        );
        assert!(
            content_text.contains("authenticate")
                || content_text.contains("hash")
                || content_text.contains("token"),
            "Should mention core authentication concepts"
        );
    } else {
        panic!("Expected final assistant message with content");
    }
}

#[tokio::test]
async fn test_searcher_generate_knowledge_documentation() {
    init_test_logging();
    let _guard = DIR_TEST_MUTEX.lock().await;

    // Create a temporary directory for this test
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let temp_path = temp_dir.path();

    // Change to the temporary directory
    std::env::set_current_dir(temp_path).expect("Failed to change directory");

    // Create a comprehensive mini web API project structure
    std::fs::create_dir_all(temp_path.join("src/api")).expect("Failed to create api directory");
    std::fs::create_dir_all(temp_path.join("src/models"))
        .expect("Failed to create models directory");
    std::fs::create_dir_all(temp_path.join("src/database"))
        .expect("Failed to create database directory");

    let api_routes_code = r#"use crate::models::User;
use crate::database::Database;

pub struct ApiRoutes {
    db: Database,
}

impl ApiRoutes {
    pub fn new(db: Database) -> Self {
        Self { db }
    }
    
    pub async fn get_users(&self) -> Vec<User> {
        self.db.get_all_users().await
    }
    
    pub async fn get_user(&self, id: u64) -> Option<User> {
        self.db.get_user_by_id(id).await
    }
    
    pub async fn create_user(&self, name: String, email: String) -> Result<User, String> {
        if !email.contains('@') {
            return Err("Invalid email format".to_string());
        }
        
        let user = User::new(0, name, email);
        self.db.create_user(user).await
    }
    
    pub async fn health_check(&self) -> &'static str {
        "API is healthy"
    }
}"#;

    let user_model_code = r#"use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: u64,
    pub name: String,
    pub email: String,
    pub created_at: String,
    pub is_active: bool,
}

impl User {
    pub fn new(id: u64, name: String, email: String) -> Self {
        Self {
            id,
            name,
            email,
            created_at: chrono::Utc::now().to_rfc3339(),
            is_active: true,
        }
    }
    
    pub fn deactivate(&mut self) {
        self.is_active = false;
    }
    
    pub fn validate_email(&self) -> bool {
        self.email.contains('@') && self.email.contains('.')
    }
}"#;

    let database_code = r#"use crate::models::User;

pub struct Database {
    connection_string: String,
}

impl Database {
    pub fn new(connection_string: String) -> Self {
        Self { connection_string }
    }
    
    pub async fn get_all_users(&self) -> Vec<User> {
        // Mock implementation
        vec![
            User::new(1, "Alice".to_string(), "alice@example.com".to_string()),
            User::new(2, "Bob".to_string(), "bob@example.com".to_string()),
        ]
    }
    
    pub async fn get_user_by_id(&self, id: u64) -> Option<User> {
        if id == 1 {
            Some(User::new(1, "Alice".to_string(), "alice@example.com".to_string()))
        } else {
            None
        }
    }
    
    pub async fn create_user(&self, mut user: User) -> Result<User, String> {
        user.id = 3; // Mock ID assignment
        Ok(user)
    }
    
    pub async fn connect(&self) -> Result<(), String> {
        // Mock connection
        Ok(())
    }
}"#;

    let main_code = r#"mod api;
mod models;
mod database;

use api::ApiRoutes;
use database::Database;

#[tokio::main]
async fn main() {
    let db = Database::new("sqlite://database.db".to_string());
    let api = ApiRoutes::new(db);
    
    println!("Health: {}", api.health_check().await);
    let users = api.get_users().await;
    println!("Users: {:?}", users);
}"#;

    std::fs::write(temp_path.join("src/api/routes.rs"), api_routes_code)
        .expect("Failed to write api routes");
    std::fs::write(
        temp_path.join("src/api/mod.rs"),
        "pub mod routes;\npub use routes::*;",
    )
    .expect("Failed to write api mod.rs");
    std::fs::write(temp_path.join("src/models/user.rs"), user_model_code)
        .expect("Failed to write user model");
    std::fs::write(
        temp_path.join("src/models/mod.rs"),
        "pub mod user;\npub use user::*;",
    )
    .expect("Failed to write models mod.rs");
    std::fs::write(temp_path.join("src/database/mod.rs"), database_code)
        .expect("Failed to write database mod.rs");
    std::fs::write(temp_path.join("src/main.rs"), main_code).expect("Failed to write main.rs");

    println!(
        "🧪 Test: Generating KNOWLEDGE.md for comprehensive API project in: {:?}",
        temp_path
    );

    // Create a searcher agent with goal to generate KNOWLEDGE.md
    let mut agent = create_searcher_agent_with_goal(
        "Generate a comprehensive KNOWLEDGE.md summary for this codebase. Include the overall architecture, key components, main functionality, file structure, and API endpoints. Focus on the layers: API, Models, and Database. Provide a clear technical overview that would help a new developer understand this project."
    ).await;

    // Run the agent
    let result = agent.run().await;

    // Verify the agent completed successfully
    assert!(
        result.is_ok(),
        "Searcher agent should complete successfully"
    );
    let agent_result = result.unwrap();
    assert!(agent_result.success, "Agent should report success");

    println!(
        "🔍 Agent completed KNOWLEDGE.md generation with {} messages",
        agent_result.trace.len()
    );

    // Check that the agent generated a proper KNOWLEDGE.md summary
    let final_message = agent_result
        .trace
        .last()
        .expect("Should have final message");
    if let ChatMessage::Assistant {
        content: Some(content),
        ..
    } = final_message
    {
        let content_text = match content {
            ChatMessageContent::Text(text) => text,
            _ => panic!("Expected text content"),
        };

        println!("📄 Generated KNOWLEDGE.md:\n{}", content_text);

        // Verify the agent generated a comprehensive summary
        assert!(
            content_text.to_uppercase().contains("KNOWLEDGE")
                || content_text.contains("# ")
                || content_text.contains("## "),
            "Should contain KNOWLEDGE.md headers or markdown formatting"
        );
        assert!(
            content_text.to_lowercase().contains("api")
                || content_text.to_lowercase().contains("endpoint"),
            "Should mention API or endpoints"
        );
        assert!(
            content_text.to_lowercase().contains("user")
                || content_text.to_lowercase().contains("model"),
            "Should mention User model or models"
        );
        assert!(
            content_text.to_lowercase().contains("database")
                || content_text.to_lowercase().contains("data"),
            "Should mention database layer"
        );
        assert!(
            content_text.contains("src/")
                || content_text.to_lowercase().contains("structure")
                || content_text.to_lowercase().contains("architecture"),
            "Should mention file structure or architecture"
        );
        assert!(
            content_text.to_lowercase().contains("component")
                || content_text.to_lowercase().contains("layer")
                || content_text.to_lowercase().contains("module"),
            "Should discuss components, layers, or modules"
        );
    } else {
        panic!("Expected final assistant message with content");
    }
}
