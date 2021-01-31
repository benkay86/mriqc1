//! Module provides OptsError replacement for [`structopt::clap::Error`], used
//! to override [`structopt::StructOpt::from_args()`] so as to propagate
//! errors through `main()` instead of calling [`std::process::exit()`].
//! Uses GitHub gist as git submodule:
//! https://gist.github.com/benkay86/6f4fbe31c219fb0a99bba13735c52206

// Allow unused code in this library-like module.
#![allow(dead_code)]

// Re-export submodule.
mod gist {
    pub mod structopt_error;
}
pub use gist::structopt_error::*;
