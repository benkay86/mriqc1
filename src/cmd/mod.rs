//! Module for command line parsing.  Uses the
//! [structopt](https://docs.rs/structopt) crate.

use std::ffi::OsString;
use std::path::PathBuf;
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
#[structopt(author, after_help=concat!("EXAMPLES:\n\t", structopt::clap::crate_name!(), " --bids-dir /path/to/pids --out-dir /path/to/out \\\n\t--parcticipant-label bob susan -- -m T1w --no-sub\n\nSEE ALSO:\n\thttps://github.com/benkay86/mriqc1\n\thttps://mriqc.org/\n\thttps://mriqc.readthedocs.io/"))]
/// Run mriqc one participant at a time, in parallel.  Specify the number of
/// parallel instances to throttle system resource usage.
pub struct Opts {
    /// BIDS directory containing data.
    #[structopt(long="bids-dir", parse(from_os_str))]
    pub bids_dir: PathBuf,

    /// Directory for output files.
    #[structopt(long="out-dir", parse(from_os_str))]
    pub out_dir: PathBuf,

    /// Participant label(s).
    #[structopt(long = "participant-label", required = true)]
    pub participant_labels: Vec<String>,

    /// Number of participants to run in parallel.
    #[structopt(short = "n", name="parallel", default_value = "1")]
    pub n_par: usize,

    /// Working directory for temporary files, defaults to system tempdir.
    #[structopt(short = "w", long = "work-dir", parse(from_os_str))]
    pub work_dir: Option<PathBuf>,

    /// Location of mriqc binary.
    #[structopt(long = "mriqc", default_value = "mriqc", env = "MRIQC", parse(from_os_str))]
    pub mriqc: PathBuf,

    /// Be quite, don't show progress bar or warnings.
    #[structopt(short = "q", long)]
    pub quiet: bool,

    /// Convert warnings about failure to process a participant to errors and
    /// exit on the first error.
    #[structopt(long)]
    pub werror: bool,

    /// Extra arguments to pass through to mriqc.
    pub extra_args: Vec<OsString>,
}

// Custom type for command line parsing errors.
mod error;
pub use error::OptsError;

impl Opts {
    /// Call this method to parse command line arguments.  Overrides default
    /// method provided by `structopt`.  See documentation for [`OptsError`] for
    /// details.
    pub fn from_args() -> Result<Opts, OptsError> {
		match Opts::from_iter_safe(std::env::args()) {
			Ok(opts) => Ok(opts),
			Err(e) => Err(OptsError { error: e })
		}
	}
}
