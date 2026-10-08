use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use tokio::sync::RwLock;

/// Represents a file system operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FsOperation {
    pub operation_type: FsOperationType,
    pub file_path: String,
    pub timestamp: DateTime<Utc>,
}

/// Types of file system operations we track
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FsOperationType {
    Read,
    Write,
    Edit,
    MultiEdit,
}

/// Shared log for tracking file system operations
#[derive(Debug)]
pub struct FsOperationLog {
    operations: RwLock<Vec<FsOperation>>,
    read_files: RwLock<HashSet<String>>, // Tracks which files have been read
    edited_files: RwLock<Vec<String>>,   // Files edited since last verification
}

impl FsOperationLog {
    /// Create a new operation log
    pub fn new() -> Self {
        Self {
            operations: RwLock::new(Vec::new()),
            read_files: RwLock::new(HashSet::new()),
            edited_files: RwLock::new(Vec::new()),
        }
    }

    /// Log a file operation
    pub async fn log_operation(&self, operation_type: FsOperationType, file_path: String) {
        let operation = FsOperation {
            operation_type: operation_type.clone(),
            file_path: file_path.clone(),
            timestamp: Utc::now(),
        };

        // Add to operations log
        {
            let mut ops = self.operations.write().await;
            ops.push(operation);
        }

        // If it's a read operation, track it in read_files
        if operation_type == FsOperationType::Read {
            let mut read_files = self.read_files.write().await;
            read_files.insert(file_path);
        } else if operation_type == FsOperationType::Edit
            || operation_type == FsOperationType::MultiEdit
        {
            let mut edited = self.edited_files.write().await;
            edited.push(file_path);
        }
    }

    /// Check if a file has been read (required before edit/multiedit)
    pub async fn has_been_read(&self, file_path: &str) -> bool {
        let read_files = self.read_files.read().await;
        read_files.contains(file_path)
    }

    /// Validate that a file can be edited (must have been read first)
    pub async fn validate_edit_permission(&self, file_path: &str) -> Result<(), String> {
        if !self.has_been_read(file_path).await {
            return Err(format!(
                "Cannot edit file '{}': The file must be read first using the Read tool before it can be edited.",
                file_path
            ));
        }
        Ok(())
    }

    /// Drain and return the list of files edited since the last call.
    /// Used by the verification system to know which files were touched.
    pub async fn drain_edited_files(&self) -> Vec<String> {
        let mut edited = self.edited_files.write().await;
        std::mem::take(&mut *edited)
    }

    /// Get all operations for a specific file
    pub async fn get_file_operations(&self, file_path: &str) -> Vec<FsOperation> {
        let operations = self.operations.read().await;
        operations
            .iter()
            .filter(|op| op.file_path == file_path)
            .cloned()
            .collect()
    }

    /// Get all operations
    pub async fn get_all_operations(&self) -> Vec<FsOperation> {
        let operations = self.operations.read().await;
        operations.clone()
    }

    /// Get list of all files that have been read
    pub async fn get_read_files(&self) -> HashSet<String> {
        let read_files = self.read_files.read().await;
        read_files.clone()
    }

    /// Clear the operation log (useful for testing)
    pub async fn clear(&self) {
        {
            let mut ops = self.operations.write().await;
            ops.clear();
        }
        {
            let mut read_files = self.read_files.write().await;
            read_files.clear();
        }
        {
            let mut edited = self.edited_files.write().await;
            edited.clear();
        }
    }

    /// Get summary statistics
    pub async fn get_summary(&self) -> FsOperationSummary {
        let operations = self.operations.read().await;
        let read_files = self.read_files.read().await;

        let mut read_count = 0;
        let mut write_count = 0;
        let mut edit_count = 0;
        let mut multiedit_count = 0;

        for op in operations.iter() {
            match op.operation_type {
                FsOperationType::Read => read_count += 1,
                FsOperationType::Write => write_count += 1,
                FsOperationType::Edit => edit_count += 1,
                FsOperationType::MultiEdit => multiedit_count += 1,
            }
        }

        FsOperationSummary {
            total_operations: operations.len(),
            read_count,
            write_count,
            edit_count,
            multiedit_count,
            unique_files_read: read_files.len(),
        }
    }
}

/// Summary of file system operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FsOperationSummary {
    pub total_operations: usize,
    pub read_count: usize,
    pub write_count: usize,
    pub edit_count: usize,
    pub multiedit_count: usize,
    pub unique_files_read: usize,
}

impl Default for FsOperationLog {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_new_log_is_empty() {
        let log = FsOperationLog::new();
        let operations = log.get_all_operations().await;
        let read_files = log.get_read_files().await;

        assert!(operations.is_empty());
        assert!(read_files.is_empty());
    }

    #[tokio::test]
    async fn test_log_read_operation() {
        let log = FsOperationLog::new();
        log.log_operation(FsOperationType::Read, "test.txt".to_string())
            .await;

        assert!(log.has_been_read("test.txt").await);
        assert!(!log.has_been_read("other.txt").await);

        let operations = log.get_all_operations().await;
        assert_eq!(operations.len(), 1);
        assert_eq!(operations[0].operation_type, FsOperationType::Read);
        assert_eq!(operations[0].file_path, "test.txt");
    }

    #[tokio::test]
    async fn test_validate_edit_permission() {
        let log = FsOperationLog::new();

        // Should fail before reading
        let result = log.validate_edit_permission("test.txt").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("must be read first"));

        // Should succeed after reading
        log.log_operation(FsOperationType::Read, "test.txt".to_string())
            .await;
        let result = log.validate_edit_permission("test.txt").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_multiple_operations() {
        let log = FsOperationLog::new();

        // Log multiple operations
        log.log_operation(FsOperationType::Read, "file1.txt".to_string())
            .await;
        log.log_operation(FsOperationType::Edit, "file1.txt".to_string())
            .await;
        log.log_operation(FsOperationType::Write, "file2.txt".to_string())
            .await;
        log.log_operation(FsOperationType::MultiEdit, "file1.txt".to_string())
            .await;

        let operations = log.get_all_operations().await;
        assert_eq!(operations.len(), 4);

        let file1_ops = log.get_file_operations("file1.txt").await;
        assert_eq!(file1_ops.len(), 3);

        let summary = log.get_summary().await;
        assert_eq!(summary.total_operations, 4);
        assert_eq!(summary.read_count, 1);
        assert_eq!(summary.edit_count, 1);
        assert_eq!(summary.write_count, 1);
        assert_eq!(summary.multiedit_count, 1);
        assert_eq!(summary.unique_files_read, 1);
    }

    #[tokio::test]
    async fn test_clear_log() {
        let log = FsOperationLog::new();

        log.log_operation(FsOperationType::Read, "test.txt".to_string())
            .await;
        assert!(!log.get_all_operations().await.is_empty());
        assert!(log.has_been_read("test.txt").await);

        log.clear().await;
        assert!(log.get_all_operations().await.is_empty());
        assert!(!log.has_been_read("test.txt").await);
    }
}
