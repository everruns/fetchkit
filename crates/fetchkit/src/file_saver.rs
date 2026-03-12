//! File saving abstractions for FetchKit
//!
//! Consumers implement [`FileSaver`] to control where fetched bytes land:
//! - CLI: writes to real filesystem ([`LocalFileSaver`])
//! - Everruns: writes to session virtual filesystem
//! - Tests: in-memory buffer

use async_trait::async_trait;
use std::path::{Component, Path, PathBuf};
use thiserror::Error;

/// Result of a save operation
#[derive(Debug, Clone)]
pub struct SaveResult {
    /// Canonical/normalized path where file was saved
    pub path: String,
    /// Bytes written
    pub bytes_written: u64,
}

/// Errors that can occur during file save operations
#[derive(Debug, Error)]
pub enum FileSaveError {
    /// Path is not allowed (traversal, outside base dir, etc.)
    #[error("Path not allowed: {0}")]
    PathNotAllowed(String),
    /// IO error during save
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    /// Other save error
    #[error("Save error: {0}")]
    Other(String),
}

/// Destination for saving fetched content to files.
///
/// Consumers implement this trait to control where bytes land:
/// - CLI: writes to real filesystem ([`LocalFileSaver`])
/// - Everruns: writes to session virtual filesystem
/// - Tests: in-memory buffer
#[async_trait]
pub trait FileSaver: Send + Sync {
    /// Save raw bytes to the given path.
    /// Returns the canonical path where the file was written and bytes written.
    async fn save(&self, path: &str, bytes: &[u8]) -> Result<SaveResult, FileSaveError>;

    /// Check if a path is writable / allowed before fetching.
    /// Default: always allowed.
    async fn validate_path(&self, path: &str) -> Result<(), FileSaveError> {
        let _ = path;
        Ok(())
    }
}

/// Saves fetched content to the real filesystem.
///
/// Ships with fetchkit as a built-in implementation for CLI usage.
///
/// # Path resolution
///
/// - With `base_dir`: relative paths resolved against it; path traversal
///   outside base_dir is rejected
/// - Without `base_dir`: only absolute paths are accepted
///
/// Parent directories are created automatically.
///
/// # Examples
///
/// ```
/// use fetchkit::LocalFileSaver;
/// use std::path::PathBuf;
///
/// // Save relative to a base directory
/// let saver = LocalFileSaver::new(Some(PathBuf::from("/tmp/downloads")));
///
/// // Save with absolute paths only
/// let saver = LocalFileSaver::new(None);
/// ```
pub struct LocalFileSaver {
    /// Optional base directory. Paths resolved relative to this.
    /// If None, paths must be absolute.
    base_dir: Option<PathBuf>,
}

impl LocalFileSaver {
    /// Create a new LocalFileSaver with an optional base directory.
    pub fn new(base_dir: Option<PathBuf>) -> Self {
        Self { base_dir }
    }

    /// Resolve and validate a path, returning the normalized absolute path.
    fn resolve_path(&self, path: &str) -> Result<PathBuf, FileSaveError> {
        let input = PathBuf::from(path);

        if let Some(base) = &self.base_dir {
            let joined = if input.is_absolute() {
                input
            } else {
                base.join(&input)
            };
            let normalized = normalize_path(&joined);
            let normalized_base = normalize_path(base);

            if !normalized.starts_with(&normalized_base) {
                return Err(FileSaveError::PathNotAllowed(format!(
                    "Path escapes base directory: {}",
                    path
                )));
            }
            Ok(normalized)
        } else {
            if !input.is_absolute() {
                return Err(FileSaveError::PathNotAllowed(
                    "Path must be absolute when no base_dir is set".into(),
                ));
            }
            Ok(normalize_path(&input))
        }
    }
}

#[async_trait]
impl FileSaver for LocalFileSaver {
    async fn save(&self, path: &str, bytes: &[u8]) -> Result<SaveResult, FileSaveError> {
        let resolved = self.resolve_path(path)?;

        // Create parent directories
        if let Some(parent) = resolved.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        tokio::fs::write(&resolved, bytes).await?;

        Ok(SaveResult {
            path: resolved.to_string_lossy().to_string(),
            bytes_written: bytes.len() as u64,
        })
    }

    async fn validate_path(&self, path: &str) -> Result<(), FileSaveError> {
        self.resolve_path(path)?;
        Ok(())
    }
}

/// Lexically normalize a path by resolving `.` and `..` components.
fn normalize_path(path: &Path) -> PathBuf {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                // Only pop if the last component is a normal component
                if matches!(components.last(), Some(Component::Normal(_))) {
                    components.pop();
                } else {
                    components.push(component);
                }
            }
            Component::CurDir => {}
            c => components.push(c),
        }
    }
    components.iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_normalize_path() {
        assert_eq!(
            normalize_path(Path::new("/a/b/../c")),
            PathBuf::from("/a/c")
        );
        assert_eq!(
            normalize_path(Path::new("/a/b/./c")),
            PathBuf::from("/a/b/c")
        );
        assert_eq!(
            normalize_path(Path::new("/a/b/c/../..")),
            PathBuf::from("/a")
        );
    }

    #[test]
    fn test_local_file_saver_resolve_relative() {
        let saver = LocalFileSaver::new(Some(PathBuf::from("/tmp/downloads")));
        let resolved = saver.resolve_path("file.txt").unwrap();
        assert_eq!(resolved, PathBuf::from("/tmp/downloads/file.txt"));
    }

    #[test]
    fn test_local_file_saver_resolve_subdirectory() {
        let saver = LocalFileSaver::new(Some(PathBuf::from("/tmp/downloads")));
        let resolved = saver.resolve_path("sub/dir/file.txt").unwrap();
        assert_eq!(resolved, PathBuf::from("/tmp/downloads/sub/dir/file.txt"));
    }

    #[test]
    fn test_local_file_saver_reject_traversal() {
        let saver = LocalFileSaver::new(Some(PathBuf::from("/tmp/downloads")));
        let result = saver.resolve_path("../../etc/passwd");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            FileSaveError::PathNotAllowed(_)
        ));
    }

    #[test]
    fn test_local_file_saver_reject_traversal_absolute() {
        let saver = LocalFileSaver::new(Some(PathBuf::from("/tmp/downloads")));
        let result = saver.resolve_path("/etc/passwd");
        assert!(result.is_err());
    }

    #[test]
    fn test_local_file_saver_no_base_requires_absolute() {
        let saver = LocalFileSaver::new(None);
        let result = saver.resolve_path("relative.txt");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            FileSaveError::PathNotAllowed(_)
        ));
    }

    #[test]
    fn test_local_file_saver_no_base_accepts_absolute() {
        let saver = LocalFileSaver::new(None);
        let resolved = saver.resolve_path("/tmp/file.txt").unwrap();
        assert_eq!(resolved, PathBuf::from("/tmp/file.txt"));
    }

    #[tokio::test]
    async fn test_local_file_saver_save_and_read() {
        let dir = tempfile::tempdir().unwrap();
        let saver = LocalFileSaver::new(Some(dir.path().to_path_buf()));

        let result = saver.save("test.txt", b"hello world").await.unwrap();
        assert_eq!(result.bytes_written, 11);
        assert!(result.path.ends_with("test.txt"));

        let content = tokio::fs::read_to_string(dir.path().join("test.txt"))
            .await
            .unwrap();
        assert_eq!(content, "hello world");
    }

    #[tokio::test]
    async fn test_local_file_saver_creates_parent_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let saver = LocalFileSaver::new(Some(dir.path().to_path_buf()));

        let result = saver
            .save("sub/dir/file.bin", &[0xFF, 0x00, 0xAB])
            .await
            .unwrap();
        assert_eq!(result.bytes_written, 3);

        let content = tokio::fs::read(dir.path().join("sub/dir/file.bin"))
            .await
            .unwrap();
        assert_eq!(content, vec![0xFF, 0x00, 0xAB]);
    }

    #[tokio::test]
    async fn test_local_file_saver_validate_path() {
        let dir = tempfile::tempdir().unwrap();
        let saver = LocalFileSaver::new(Some(dir.path().to_path_buf()));

        assert!(saver.validate_path("safe.txt").await.is_ok());
        assert!(saver.validate_path("sub/dir/safe.txt").await.is_ok());
        assert!(saver.validate_path("../../escape.txt").await.is_err());
    }
}
