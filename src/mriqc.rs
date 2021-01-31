//! This module contains tools for working with mriqc.

use crate::bids::{BidsError, BidsParticipant, ShadowBids};
use crate::cancellable_process::{CancellableChild, CancelSignal};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tempfile::TempDir;
use thiserror::Error;
use tokio::process::Command;

/// Custom error type.
#[derive(Error, Debug)]
pub enum MriqcError {
    /// Couldn't create a temporary directory within the working directory.
    #[error("Couldn't create temporary directory within working directory: {}", work_dir.to_string_lossy())]
    TempDir {
        /// Working directory in which we tried to create temporary directory.
        work_dir: PathBuf,
        source: std::io::Error
    },
    /// There was an error running the mriqc command.
    #[error("Error running mriqc.\nCommand line: {:?} {:?}", cmd, args)]
    Process {
        /// The command, e.g. `/usr/local/bin/mriqc`.
        cmd: OsString,
        /// Command line arguments.
        args: Vec<OsString>,
        source: std::io::Error
    },
    /// There was an error running the mriqc command.  Some output was captured
    /// from the command's standard output and error.
    #[error("Error running mriqc, exited with status {:?}.\nCommand line: {:?} {:?}\nOutput: {}", status, cmd, args, String::from_utf8_lossy(stderr))]
    ProcessWithOutput {
        /// The command, e.g. `/usr/local/bin/mriqc`.
        cmd: OsString,
        /// Command line arguments.
        args: Vec<OsString>,
        /// Captured output of mriqc command on stdout, if any.
        stdout: Vec<u8>,
        /// Captured output of mriqc command on stderr, if any.
        stderr: Vec<u8>,
        /// Exit status/code of the process.
        status: Option<i32>
    },
    /// There was an error setting up the shadow bids tree for this process.
    #[error(transparent)]
    BidsError(#[from] BidsError),
}

/// Options for [`Mriqc1Process::new()`]
pub struct Mriqc1Options<'a> {
    /// Root directory of BIDS tree containing participants' data.
    pub bids_dir: &'a Path,
    /// Output directory.
    pub out_dir: &'a Path,
    /// Participant id.
    pub participant: &'a str,
    /// Path to mriqc binary.  Defaults to `mriqc`.
    pub mriqc: Option<&'a Path>,
    /// Where to create temporary files.  Defaults to system temporary
    /// directory.
    pub work_dir: Option<&'a Path>,
    /// Vector of additional arguments to pass through to mriqc.
    pub extra_args: Vec<&'a OsStr>,
}

/// Resources for an instance of mriqc processing a single participant.
pub struct Mriqc1Process<F> {
    // mriqc process
    process: CancellableChild<F>,
    // BIDS filesystem resources for the participant being processed.
    _bids_participant: BidsParticipant,
    // The command, e.g. `/usr/local/bin/mriqc`.
    cmd: OsString,
    // Command line arguments.
    args: Vec<OsString>
}
impl<F: FnMut() -> Option<CancelSignal> + Unpin> Mriqc1Process<F> {
    /// Invoke an instance of mriqc to process one participant with the provided
    /// `options`.  The closure `cancel` is called periodically, and if returns
    /// some [`CancelSignal`] then then this instance of mriqc will be cancelled
    /// (i.e. interrupted, aborted); return `None` from the closure to continue
    /// processing.
    pub async fn new_with_cancel(options: Mriqc1Options<'_>, cancel: F) -> Result<Self, MriqcError> {
        // Destructure options and set default values.
        let bids_dir = options.bids_dir;
        let out_dir = options.out_dir;
        let participant = options.participant;
        let mriqc = options.mriqc.unwrap_or(Path::new("mriqc"));
        let work_dir = match options.work_dir {
            Some(work_dir) => work_dir.into(),
            None => std::env::temp_dir()
        };
        let extra_args = options.extra_args;

        // Set up the shadow BIDS tree.
        // Create a unique temporary directory within the working directory with
        // a randomly assigned name.
        let temp_dir = Arc::new(TempDir::new_in(&work_dir).map_err(|source|
            MriqcError::TempDir{work_dir, source}
        )?);
        // Create the shadow BIDS tree in the temporary directory.
        let shadow_bids = Arc::new(ShadowBids::new_with_parent(bids_dir, temp_dir.clone()).await?);
        let shadow_bids_path = shadow_bids.path();
        // Register the BIDS participant within the shadow BIDS tree.
        let bids_participant = BidsParticipant::new(participant, shadow_bids.clone()).await?;

        // Spawn the mriqc process.
        // Compose command line arguments.
        let args = {
            // Mandary command line arguments.
            let mut args: Vec<OsString> = vec![
                shadow_bids_path.as_os_str().into(), // BIDS tree
                out_dir.as_os_str().into(), // output directory
                OsStr::new("participant").into(), // do participant-level analysis
                OsStr::new("--work-dir").into(), temp_dir.path().as_os_str().into(), // use temporary directory as working directory for this instance of mriqc
                OsStr::new("--participant-label").into(), OsStr::new(participant).into() // specify one participant label, correponding to this one participant we want to process
            ];
            // Append extra arguments.
            args.extend(extra_args.into_iter().map(|arg| arg.into()));
            args
        };
        // Build the command and spawn the process.
        let process = Command::new(mriqc)
            .args(&args)
            .stdin(std::process::Stdio::null()) // no keyboard input to process
            .stdout(std::process::Stdio::piped()) // capture stdout
            .stderr(std::process::Stdio::piped()) // capture stderr
            .current_dir(temp_dir.path()) // make working directory this instance's temporary directory
            .kill_on_drop(true) // if this object is dropped mriqc's resources will be destroyed, so we should kill the process
            .spawn() // fire it up!
            .map_err(|source| // wrap error in context
                MriqcError::Process {
                    cmd: mriqc.into(),
                    args: args.clone(),
                    source // cause of this error
                }
            )?;
        // Wrap inside a CancellableChild.
        let process = CancellableChild::new(process, cancel);

        // Construct self.
        Ok(Mriqc1Process {
            process,
            _bids_participant: bids_participant,
            cmd: mriqc.into(),
            args
        })
    }
    /// Wait for this mriqc process to finish, or for the process to be
    /// cancelled via its cancel closure (see
    /// [`Mriqc1Process::new_with_cancel`]), whichever comes first.  If the
    /// process finished successfully or if it was cancelled returns `Ok(())`.
    /// Otherwise returns an error.
    pub async fn wait(self) -> Result<(), MriqcError> {
        match self.process.wait_with_output().await {
            // We successfully waited.
            Ok(output) => match output.how_cancelled {
                // The child was cancelled.  Return sucecss.
                Some(_) => Ok(()),
                // The child wasn't cancelled.  Inspect the output.
                None => {
                    // If child was not cancelled then unwrap() is guaranteed
                    // not to panic.
                    let output = output.output.unwrap();
                    match output.status.success() {
                        // The child finished succesfully.  Return success.
                        true => Ok(()),
                        // There was an error, but we have some output to help`
                        // figure out what happened.
                        false => Err(MriqcError::ProcessWithOutput {
                            cmd: self.cmd,
                            args: self.args,
                            stdout: output.stdout,
                            stderr: output.stderr,
                            status: output.status.code()
                        })
                    }
                }
            },
            // An error happened and we didn't get any output.
            Err(source) => Err(MriqcError::Process {
                cmd: self.cmd,
                args: self.args,
                source
            })
        }
    }
}
impl Mriqc1Process<fn() -> Option<CancelSignal>> {
    /// Convenience constructor to create a new `Mriqc1Process` that cannot be
    /// cancelled.  See documentation for [`Mriqc1Process::new_with_cancel()`].
    pub async fn new(options: Mriqc1Options<'_>) -> Result<Self, MriqcError> {
        Self::new_with_cancel(options, never_cancel).await
    }
}

// Default cancel closure (actually function pointer) for Mriqc1Process::new().
fn never_cancel() -> Option<CancelSignal> {
    None
}
