//! Additional temporary filesystem structures to supplement [`tempfile`].

// Note the use of synchronous filesystem operations in destructor pending
// support for async Drop in Rust: https://boats.gitlab.io/blog/post/poll-drop/

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use thiserror::Error;

/// Custom error type adds the offending path to [`std::io::Error`].
#[derive(Error, Debug)]
pub enum FileSystemError {
    /// Directory creation failed.
    #[error("Could not create: {}", path.to_string_lossy())]
    DirCreateError {
        path: PathBuf,
        source: tokio::io::Error
    },
    /// Directory removal failed.
    #[error("Could not remove: {}", path.to_string_lossy())]
    DirRemoveError {
        path: PathBuf,
        source: tokio::io::Error
    },
    /// Symlink creation failed.
    #[error("Could not create symlink \"{}\" to \"{}\".", dst_path.to_string_lossy(), src_path.to_string_lossy())]
    SymlinkCreateError {
        src_path: PathBuf,
        dst_path: PathBuf,
        source: tokio::io::Error
    },
    /// Symlink removal failed.
    #[error("Could not remove symlink \"{}\" to \"{}\".", dst_path.to_string_lossy(), src_path.to_string_lossy())]
    SymlinkRemoveError {
        src_path: PathBuf,
        dst_path: PathBuf,
        source: tokio::io::Error
    }
}

/// Temporary directories of type [`tempfile::TempDir`] have a randomly-
/// assigned name.  This type represents a temporary directory with a user-
/// assignable name.  The directory and all of its contents will be deleted
/// when the `NamedTempDir` object is dropped.
pub struct NamedTempDir {
    // Has close() or close_all() been called?
    closed: bool,
    // Filesystem path of owned directory.
    path: PathBuf
}
impl NamedTempDir {
    /// Close and remove the temporary directory, but only if it is empty.
    pub async fn close(&mut self) -> Result<(), FileSystemError> {
        if !self.closed {
            tokio::fs::remove_dir(&self.path).await.map_err( |source|
                FileSystemError::DirRemoveError {
                    path: self.path.clone(), source
                }
            )?;
            self.closed = true;
        }
        Ok(())
    }

    /// Close and remove the temporary directory and all its contents.
    pub async fn close_all(&mut self) -> Result<(), FileSystemError> {
        if !self.closed {
            tokio::fs::remove_dir_all(&self.path).await.map_err( |source|
                FileSystemError::DirRemoveError {
                    path: self.path.clone(), source
                }
            )?;
            self.closed = true;
        }
        Ok(())
    }

    /// Check if close() or close_all() has been called.
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// Create a new temporary directory at the given path.
    pub async fn new<P: Into<PathBuf>>(path: P) -> Result<Self, FileSystemError> {
        let path = path.into();
        match tokio::fs::create_dir(&path).await {
            Err(source) => Err(FileSystemError::DirCreateError {
                path, source
            }),
            Ok(_) => Ok(Self {
                closed: false, path
            })
        }
    }

    /// Get the filesystem path for this temporary directory.
    pub fn path(&self) -> &Path {
        &self.path
    }
}
impl Drop for NamedTempDir {
    fn drop(&mut self) {
        if !self.closed {
            // On destruction, remove the corresponding filesystem directory.
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

/// Temporary symlink.
pub struct TempSymlink {
    // Has close() or close_all() been called?
    closed: bool,
    // Filesystem path to which this symlink points.
    src_path: PathBuf,
    // Filesystem path of this symlink.
    dst_path: PathBuf
}
impl TempSymlink {
    /// Close and remove the temporary directory, but only if it is empty.
    pub async fn close(&mut self) -> Result<(), FileSystemError> {
        if !self.closed {
            tokio::fs::remove_file(&self.dst_path).await.map_err( |source|
                FileSystemError::SymlinkRemoveError {
                    src_path: self.src_path.clone(),
                    dst_path: self.dst_path.clone(),
                    source
                }
            )?;
            self.closed = true;
        }
        Ok(())
    }

    /// Get the filesystem path of this symlink.
    pub fn dst_path(&self) -> &Path {
        &self.dst_path
    }

    /// Check if close() or close_all() has been called.
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// Create a new temporary symlink from the path `src` to `dst`.
    pub async fn new<P1: Into<PathBuf>, P2: Into<PathBuf>>(src: P1, dst: P2) -> Result<Self, FileSystemError> {
        let src = src.into();
        let dst = dst.into();
        match tokio::fs::symlink(&src, &dst).await {
            Err(source) => Err(FileSystemError::SymlinkCreateError {
                src_path: src,
                dst_path: dst,
                source
            }),
            Ok(_) => Ok(Self {
                closed: false,
                src_path: src,
                dst_path: dst
            })
        }
    }

    /// Get the filesystem path to which this symlink points.
    pub fn src_path(&self) -> &Path {
        &self.src_path
    }
}
impl Drop for TempSymlink {
    fn drop(&mut self) {
        if !self.closed {
            // On destruction, remove the corresponding filesystem directory.
            let _ = std::fs::remove_file(&self.dst_path);
        }
    }
}
