use anyhow::{bail, Context, Result};
use futures::stream::{StreamExt, TryStreamExt};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use mriqc1::cancellable_process::CancelSignal;
use mriqc1::mriqc::{MriqcError, Mriqc1Options, Mriqc1Process};
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::io::AsyncWriteExt;

mod cmd;

#[tokio::main]
async fn main() -> Result<()> {
    // Parse command line arguments and destructure.
    let cmd_opts = cmd::Opts::from_args()?;
    let cmd_opts_quiet = cmd_opts.quiet;
    let cmd_opts_n_par = cmd_opts.n_par;
    let cmd_opts_werror = cmd_opts.werror;
    let participants = cmd_opts.participant_labels;
    struct MriqcOptions { // pptions passed to each instance of mriqc
        bids_dir: PathBuf,
        out_dir: PathBuf,
        mriqc: PathBuf,
        work_dir: Option<PathBuf>,
        extra_args: Vec<OsString>
    }
    let mriqc_options = Arc::new(MriqcOptions {
        bids_dir: cmd_opts.bids_dir,
        out_dir: cmd_opts.out_dir,
        mriqc: cmd_opts.mriqc,
        work_dir: match cmd_opts.work_dir {
            Some(work_dir) => Some(work_dir.into()),
            None => Some(std::env::temp_dir())
        },
        extra_args: cmd_opts.extra_args
    });

    // Make sure provided paths are valid, readable/writable directories.
    // Can we read from the BIDS directory?
    let _ = tokio::fs::read_dir(&mriqc_options.bids_dir).await.context(format!("Couldn't read BIDS directory: {}", mriqc_options.bids_dir.to_string_lossy()))?;
    // Can we write to the output directory?
    { let _ = tempfile::tempdir_in(&mriqc_options.out_dir).context(format!("Output directory is not writable: {}", mriqc_options.out_dir.to_string_lossy()))?; }
    // TODO Can we write to the working directory?
    if let Some(ref work_dir) = mriqc_options.work_dir {
        let _ = tempfile::tempdir_in(work_dir).context(format!("Working directory is not writable: {}", work_dir.to_string_lossy()))?;
    }

    // Set up a multi-progress bar.
    // The bar is stored in an `Arc` to facilitate sharing between threads.
    let multibar = std::sync::Arc::new(MultiProgress::new());
    // Create an overall progress indicator.
    let main_pb = match cmd_opts_quiet {
        // Sshhh... hide the progress bar if user asked us to be quite!
        true => ProgressBar::hidden(),
        // Default, visible progress bar.
        false => {
            // Lead with message on stderr.
            {
                let mut stderr = tokio::io::stderr();
                stderr.write_all(b"Running mriqc, this could take a long time. press Ctrl+C to cancel...\n").await?;
            }
            // Configure progress bar.
            let pb = ProgressBar::new(participants.len() as u64)
            .with_style(
                ProgressStyle::default_bar()
		        .template("({pos}/{len} participants): {elapsed} [{wide_bar}] {eta}")
		        .progress_chars("=> ")
            );
            pb
        }
    };
    // Add this indicator to the multibar.
    let main_pb = Arc::new(multibar.clone().add(main_pb));
    // Tick the bar once now so it will render above the participants' spinner
    // bars.
    main_pb.tick();
    // Animate progress bars on a separate thread.
    let multibar_animation = {
        // Create a clone of the multibar, which we will move into the task.
        let multibar = multibar.clone();

        // multibar.join() is *not* async and will block until all the progress
        // bars are done, therefore we must spawn it on a separate scheduler
        // on which blocking behavior is allowed.
        tokio::task::spawn_blocking(move || { multibar.join() })
    };

    // Install signal handler.  Set atomic flag to true if we are interrupted.
    let interrupted = Arc::new(AtomicBool::new(false));
    {
        let interrupted = interrupted.clone();
        tokio::spawn(async move {
            // Wait for the interrupt signal in a separate thread.  We do not ever
            // have to join this thread.  It will get cleaned up when the program
            // terminates.
            tokio::signal::ctrl_c().await.expect("Failed to listen for interrupt signal.");

            // Received interrupt signal, set global interrupt flag.
            interrupted.store(true, Ordering::Relaxed);
        });
    }

    // Iterate over stream of participants provded on the command line.
    futures::stream::iter(participants)

        // Cancel the stream if we get interrupted.
        .take_while(|_| {
            let interrupted = interrupted.clone();
            async move { !interrupted.load(Ordering::Relaxed) }
        })
        // Perform the actual mriqc processing.
        .then(|participant| {
            // Set up a progress bar for this participant.
            let participant_pb = ProgressBar::new_spinner()
            .with_style( // set style on progress bar
                ProgressStyle::default_spinner()
	            .template("Running mriqc on participant {msg} {spinner}")
            );
            participant_pb.set_message(&participant);
            participant_pb.enable_steady_tick(2000); // spin every 2 seconds
            let participant_pb = multibar.clone().add(participant_pb);
            // Clone references we need to move into async block.
            let main_pb = main_pb.clone();
            let interrupted = interrupted.clone();
            let mriqc_options = mriqc_options.clone();
            // Spawn mriqc for this participant and update progress bar.
            async move {
                // Await result of mriqc.
                let res = async move {
                    let options = Mriqc1Options {
                        bids_dir: &mriqc_options.bids_dir,
                        out_dir: &mriqc_options.out_dir,
                        mriqc: Some(&mriqc_options.mriqc),
                        work_dir: mriqc_options.work_dir.as_deref(),
                        extra_args: mriqc_options.extra_args.iter().map(|s| s as &OsStr).collect(),
                        participant: &participant
                    };
                    // Closure to interrupt the mriqc process.
                    let cancel = || match interrupted.load(Ordering::Relaxed) {
                        true => Some(CancelSignal::Interrupt),
                        false => None
                    };
                    // Spawn the mriqc process.
                    let process = Mriqc1Process::new_with_cancel(options, cancel).await?;
                    // Wait for it to either finish or be cancelled.
                    process.wait().await?;
                    // Make return type of Result<(), MriqcError> explicit.
                    Ok::<(), MriqcError>(())
                }.await;
                // Update progress bar before propagating errors.
                // Finish this participant's progress bar.
                participant_pb.finish_and_clear();
                // Increment main progress bar.
                main_pb.inc(1);
                // Now we can propagate any errors.
                res.and(Ok(()))
            }
        })
        // Emit warnings and filter them out of the stream.
        .filter(|result| match cmd_opts_werror {
            // Don't convert warnings to errors.  Pass them through as errors.
            // This will cause the stream to stop after encountering the first
            // error.
            true => futures::future::ready(true),
            // Emit warnings and return false to filter them out.
            false => match result {
                Err(warning) => {
                    eprintln!("Warning: {}", warning);
                    futures::future::ready(false)
                },
                // Pass through successful results.
                Ok(_) => futures::future::ready(true)
            }
        })
        // Process up to N files concurrently.  Cancel stream early on any
        // unfiltered errors that have propagated to this point.
        .try_for_each_concurrent(cmd_opts_n_par, |_| std::future::ready(Ok(())))
        .await?;

    // Wait for progress bar to join.
    // First ? for outer join of tokio::task
    // Second ? for MultiProgress::join()
    multibar_animation.await??;

    // Detect if we were interrupted.
    if interrupted.load(Ordering::Acquire) {
        bail!("Process interrupted by SIGINT.");
    }

    // All done!
    if !cmd_opts_quiet {
        let mut stderr = tokio::io::stderr();
        stderr.write_all(b"...all done.\n").await?;
    }
    Ok(())
}
