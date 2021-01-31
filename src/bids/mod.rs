//! Module containing tools to work with Brain Imaging Data Structure (BIDS)
//! files and folders.  This is not meant to be an exhaustive Rust library for
//! working with BIDS data.  It just implements the functionality needed for the
//! mriqc1 application.
//!
//! The purpose of these types it gracefully shadow a BIDS tree containing many
//! subject into an identical BIDS tree containing just a few subjects.
//!
//! The [`ShadowBids`] type "shadows" a real BIDS tree using symlinks.  It owns
//! symlinks to participant non-specific files such as dataset_description.json,
//! etc.  The symlinks and shadow bids directory itself are automatically
//! cleaned up when the `ShadowBids` instance is dropped.  You can create
//! symlinks to one or more participants inside the shadowed BIDS tree using
//! a [`BidsParticipant`] for each subject.  These symlinks are also
//! automatically cleaned up when dropped.  Each instance of `BidsParticipant`
//! holds a reference to its parent `ShadowBids` such that the `ShadowBids` is
//! not dropped until all of its child `BidsParticipant`s are dropped.
//!
//! See https://bids.neuroimaging.io/

use std::path::{Path, PathBuf};
use std::sync::Arc;
use tempfile::TempDir;
use thiserror::Error;

mod temp;
use temp::{FileSystemError, NamedTempDir, TempSymlink};

/// Enumeration over bids-related errors.
#[derive(Error, Debug)]
pub enum BidsError {
    /// Tried to create a [`ShadowBids`] inside a parent temporary directory,
    /// but the provided destonation path already has a parent.
    #[error("Destination path \"{}\" already has a parent.", path.to_string_lossy())]
    HasParent {
        path: PathBuf,
    },
    /// Tried to create a [`BidsParticipant`] using a participant ID that does
    /// not exist in the given [`ShadowBids`] tree.
    #[error("BIDS tree \"{}\" is missing participant \"{}\"", bids_src.to_string_lossy(), participant)]
    MissingParticipant {
        bids_src: PathBuf,
        participant: String
    },
    /// Couldn't canonicalize the path to the BIDS tree, or else the BIDS tree
    /// is the filesystem root (which should not happen).
    #[error("Couldn't canonicalize path to BIDS tree: {}", bids_src.to_string_lossy())]
    Canonicalize {
        bids_src: PathBuf,
        source: Option<std::io::Error>,
    },
    /// There was an error performing a filesystem operation.
    #[error(transparent)]
    FileSystem(#[from] FileSystemError),
}

/// Fake BIDS data structure that shadows a real BIDS data structure.  Intended
/// to contain one or more [`BidsLink`] symlinks to participant's BIDS-formatted
/// data.
pub struct ShadowBids {
    // Hold reference to parent temporary directory.
    // TempDir is deleted when there are no more references to it.
    parent: Option<Arc<TempDir>>,
    // Path to BIDS tree which this BIDS tree is shadowing.
    src: PathBuf,
    // Path to this directory.
    // Directory will be deleted when this ShadowBids instance is dropped.
    path: NamedTempDir,
    // Symlink to dataset description, which may or may not exist.
    _dataset_description: Option<TempSymlink>,
    // Symlink to sourcedata directory, which may or may not exist.
    _sourcedata: Option<TempSymlink>,
    // Symlink to participants.tsv file, which may or may not exist.
    _participants_tsv: Option<TempSymlink>
}
impl ShadowBids {
    /// Create a new shadow bids tree from the real bids tree located at `src`.
    /// The shadow bids tree will be created at the path `dst`.  If a parent
    /// temporary directory is provided then `dst` will be relative to `parent`
    /// and must not contain `/`.
    pub async fn new<P1: Into<PathBuf>, P2: Into<PathBuf>>(src: P1, dst: P2, parent: Option<Arc<TempDir>>) -> Result<Self, BidsError> {
        let src = src.into();
        let dst = dst.into();

        // Determine destination path depending on whether there is a parent
        // directory.
        let dst: PathBuf = match &parent {
            Some(parent) => {
                // Verify that dst does not already have a parent.
                if !dst.is_relative() || dst.parent().is_some() {
                    return Err(BidsError::HasParent{path: dst});
                }
                // Place dst inside parent.
                parent.path().join(dst)
            }
            None => dst
        };

        // Create the shadow bids directory.
        let dst = NamedTempDir::new(dst).await?;

        // Create a symlink to the dataset_description.json file, if it exists.
        let dataset_description = {
            let src_dataset_description = src.join("dataset_description.json");
            match exists(&src_dataset_description).await {
                true => Some(TempSymlink::new(src_dataset_description, dst.path().join("dataset_description.json")).await?),
                false => None
            }
        };

        // Create a symlink to the sourcedata directory, if it exists.
        let sourcedata = {
            let src_sourcedata = src.join("sourcedata");
            match exists(&src_sourcedata).await {
                true => Some(TempSymlink::new(src_sourcedata, dst.path().join("sourcedata")).await?),
                false => None
            }
        };

        // Create a symlink to the participants.tsv file, if it exists.
        let participants_tsv = {
            let src_participants_tsv = src.join("participants.tsv");
            match exists(&src_participants_tsv).await {
                true => Some(TempSymlink::new(src_participants_tsv, dst.path().join("participants.tsv")).await?),
                false => None
            }
        };

        // Compose self.
        Ok(Self {
            parent,
            src,
            path: dst,
            _dataset_description: dataset_description,
            _sourcedata: sourcedata,
            _participants_tsv: participants_tsv
        })
    }

    /// Create a new shadow bids tree from the real bids tree located at `src`.
    /// The root of the shadow bids tree will be located at `parent/src`.
    pub async fn new_with_parent<P1: Into<PathBuf>>(src: P1, parent: Arc<TempDir>) -> Result<Self, BidsError> {
        let src = src.into();
        let dst: PathBuf = match src.canonicalize() {
            Ok(path) => match path.file_name() {
                Some(name) => Ok(name.to_os_string()),
                None => Err(BidsError::Canonicalize {
                    bids_src: src.clone(),
                    source: None
                })
            },
            Err(source) => Err(BidsError::Canonicalize {
                bids_src: src.clone(),
                source: Some(source)
            })
        }?.into();
        Self::new(src, dst, Some(parent)).await
    }

    /// Get parent temporary directory, if one exists.
    /// Get parent BIDS directory tree root for this participant.
    pub fn parent(&self) -> Option<Arc<TempDir>> {
        self.parent.clone()
    }

    /// Get root path of this shadow BID tree.
    pub fn path(&self) -> &Path {
        self.path.path()
    }

    /// Get root path of source BIDS tree.
    pub fn src(&self) -> &Path {
        &self.src
    }
}

/// Symlinks to a participant's BIDS-formatted data.
pub struct BidsParticipant {
    // Hold reference to parent BIDS tree.
    // ShadowBids is deleted when there are no more references to it.
    parent: Arc<ShadowBids>,
    // Path to this symlink.
    // Symlink will be removed when this BidsLink instance is dropped.
    path: TempSymlink,
}
impl BidsParticipant {
    /// Create a new symlink to a BIDS participant inside a parent BIDS tree.
    pub async fn new<S: AsRef<str>>(participant: S, parent: Arc<ShadowBids>) -> Result<Self, BidsError> {
        let participant = participant.as_ref();

        // Does the participant exist within the parent BIDS tree?
        let sub_str = format!("sub-{}", participant);
        let src = parent.src().join(&sub_str);
        match exists(&src).await {
            false => Err(BidsError::MissingParticipant{
                bids_src: parent.src().into(),
                participant: participant.into()
            }),
            true => {
                let dst = parent.path().join(sub_str);
                Ok(Self {
                    parent,
                    path: TempSymlink::new(src, dst).await?
                })
            }
        }
    }
    /// Get parent BIDS directory tree root for this participant.
    pub fn parent(&self) -> Arc<ShadowBids> {
        self.parent.clone()
    }
    /// Get path to root of this participant's data within the shadow bids tree.
    pub fn path(&self) -> &Path {
        self.path.dst_path()
    }
    /// Get path to root of this participant's data within the source bids tree.
    pub fn src(&self) -> &Path {
        self.path.src_path()
    }
}

// Check if path exists.
async fn exists<P: AsRef<Path>>(path: P) -> bool {
    tokio::fs::metadata(path.as_ref()).await.is_ok()
}
