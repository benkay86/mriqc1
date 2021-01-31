//! Module to attach am indicatif progress bar to a [futures::stream::Stream]
//! using `ProgressStream::progress_with()`, analagous to using
//! `ProgressIterator::progress_with()` on an iterator.
//! Uses GitHub gist as git submodule:
//! https://gist.github.com/benkay86/6afffd4cf90ad84ac43e42d59d197e08

// Allow unused code in this library-like module.
#![allow(dead_code)]

// Re-export submodule.
mod gist {
    pub mod indicatif_progress_stream;
}
pub use gist::indicatif_progress_stream::*;
