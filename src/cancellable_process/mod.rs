//! Wraps a [`tokio::process::Child`] in a [`CancellableChild`] which can be
//! cancelled asynchronously using a closure while `wait()`ing for it to finish.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::process::{Child, ChildStdin, ChildStdout, ChildStderr};

/// How to signal cancellation to a child process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelSignal {
    /// On unix platforms, send the child process SIGINT.
    Interrupt,
    /// On unix platforms, send the child process SIGKILL.
    Kill
}

/// Exit status of a completed child process.
#[derive(Debug, Clone, Copy)]
pub struct ExitStatus {
    /// How the process was cancelled, or `None` of the process was not
    /// cancelled.
    pub how_cancelled: Option<CancelSignal>,
    /// Exit status of the process.  May be `None` if child was cancelled but
    /// has not yet exited.  Guaranteed to be `Some` if `how_cancelled` is
    /// `None`.
    pub status: Option<std::process::ExitStatus>
}

/// Output of a completed child process.
#[derive(Debug, Clone)]
pub struct Output {
    /// How the process was cancelled, or `None` of the process was not
    /// cancelled.
    pub how_cancelled: Option<CancelSignal>,
    /// Output of the process.  May be `None` if child was cancelled but has not
    /// yet exited.  Guaranteed to be `Some` if `how_cancelled` is `None`.
    pub output: Option<std::process::Output>
}

/// Structure representing a [`tokio::process::Child`] that can be cancelled
/// while asynchronously waiting for it to finish.
#[derive(Debug)]
pub struct CancellableChild<F> {
    /// See [`tokio::process::Child::stdin`].
    pub stdin: Option<ChildStdin>,
    /// See [`tokio::process::Child::stdout`].
    pub stdout: Option<ChildStdout>,
    /// See [`tokio::process::Child::stderr`].
    pub stderr: Option<ChildStderr>,
    // The wrapped child process.
    child: Child,
    // Closure that checks whether and how to cancel process.
    check_cancel: F,
    // How the child process was cancelled, or None if it was not cancelled.
    how_cancelled: Option<CancelSignal>,
    // The child process's exit status, or None if it is not finished.
    exit_status: Option<std::process::ExitStatus>
}
impl<F: FnMut() -> Option<CancelSignal> + Unpin> CancellableChild<F> {
    /// Create a new `CancelChild` from an existing [`tokio::process::Child`]
    /// and a closure that is called periodically to check whether the child
    /// process should be cancelled.  The closure takes no arguments and must
    /// return a [`CancelSignal`] specifying which signal to cancel the child
    /// process with, or else `None` if the child process should not be
    /// cancelled.
    pub fn new(child: Child, f: F) -> Self {
        let mut child = child;
        let stdin = child.stdin.take();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        Self {
            stdin, stdout, stderr, child,
            check_cancel: f,
            how_cancelled: None,
            exit_status: None
        }
    }
    /// See [`tokio::process::Child::id()`].
    pub fn id(&self) -> Option<u32> {
        self.child.id()
    }
    /// See [`tokio::process::Child::start_kill()`].
    pub fn start_kill(&mut self) -> std::io::Result<()> {
        self.child.start_kill()
    }
    /// See [`tokio::process::Child::kill()`].
    pub async fn kill(&mut self) -> std::io::Result<()> {
        self.child.kill().await
    }
    /// Similar to ['tokio::process::Child::wait()`], but the returned `Future`
    /// will cancel the process and resolve immediately if the cancellation
    /// closure provided to [`CancellableChild::new()`] returns some
    /// [`CancelSignal`].
    pub fn wait(&mut self) -> ChildWaitFuture<'_, F, impl '_ + Future<Output = std::io::Result<std::process::ExitStatus>>> {
        // Destructure, then create future.
        let id = self.id();
        let check_cancel = &mut self.check_cancel;
        let how_cancelled = &mut self.how_cancelled;
        let exit_status = &mut self.exit_status;
        let fut = Box::pin(self.child.wait());
        ChildWaitFuture {
            id,
            check_cancel,
            how_cancelled,
            exit_status,
            fut
        }
    }
    /// See [`tokio::process::Child::try_wait()`].
    pub fn try_wait(&mut self) -> std::io::Result<Option<ExitStatus>> {
        self.exit_status = self.child.try_wait()?;
        Ok(self.exit_status.map(|status| ExitStatus {
            how_cancelled: self.how_cancelled,
            status: Some(status)
        }))
    }
    /// See [tokio::process::Child::wait_with_output()`] and
    /// and [`CancellableChild::wait()`].
    pub fn wait_with_output(self) -> ChildWaitOutputFuture<F, impl Future<Output = std::io::Result<std::process::Output>>> {
        // Destructure.
        let id = self.id();
        let check_cancel = self.check_cancel;
        let how_cancelled = self.how_cancelled;
        let mut child = self.child;
        // Put i/o back in child.
        child.stdin = self.stdin;
        child.stdout = self.stdout;
        child.stderr = self.stderr;
        // Create future.
        let fut = Box::pin(child.wait_with_output());
        ChildWaitOutputFuture {
            id,
            check_cancel,
            how_cancelled,
            fut
        }
    }
    /// Consume this `CancellableChild` and get the inner
    /// [`tokio::process::Child`].
    pub fn into_child(self) -> Child {
        let mut child = self.child;
        child.stdin = self.stdin;
        child.stdout = self.stdout;
        child.stderr = self.stderr;
        child
    }
}

/// Future returned by [`CancellableChild::wait()`].  This future will finish
/// when the child process has exited or if the child process has been
/// cancelled, whichever comes first.
pub struct ChildWaitFuture<'child, F: FnMut() -> Option<CancelSignal>, Fut: 'child + Future<Output = std::io::Result<std::process::ExitStatus>>> {
    id: Option<u32>,
    check_cancel: &'child mut F,
    how_cancelled: &'child mut Option<CancelSignal>,
    exit_status: &'child mut Option<std::process::ExitStatus>,
    fut: Pin<Box<Fut>>,
}
impl<'child, F: FnMut() -> Option<CancelSignal>, Fut: 'child + Future<Output = std::io::Result<std::process::ExitStatus>>> Future for ChildWaitFuture<'child, F, Fut> {
    type Output = std::io::Result<ExitStatus>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // Need mutable self.
        let this = self.get_mut();

        // First check if the child process has already exited.
        if let Some(exit_status) = this.exit_status {
            return Poll::Ready(Ok(ExitStatus {
                how_cancelled: *this.how_cancelled,
                status: Some(*exit_status)
            }));
        }

        // Check if the child process is being cancelled.
        let cancel_signal = (this.check_cancel)();

        // Poll the future.
        let poll_result = this.fut.as_mut().poll(cx);

        // Deal with result.
        match poll_result {
            // The child has finished.  Hooray!
            Poll::Ready(status) => match status {
                Ok(status) => {
                    *this.exit_status = Some(status);
                    Poll::Ready(Ok(ExitStatus {
                        how_cancelled: *this.how_cancelled,
                        status: Some(status)
                    }))
                },
                Err(e) => Poll::Ready(Err(e))
            },
            // The child has not yet finished.
            Poll::Pending => {
                // Remember how we were cancelled.
                *this.how_cancelled = cancel_signal;
                match cancel_signal {
                    // Cancel the child process and become ready immediately.
                    Some(cancel_signal) => match cancel_signal {
                        CancelSignal::Interrupt => {
                            // Interrupt the child process.
                            if let Some(id) = this.id {
                                unsafe {
                                    // Unsafe because we need to call libc, and
                                    // because process id may be stale.
                                    libc::kill(id as i32, libc::SIGINT);
                                }
                            }
                            Poll::Ready(Ok(ExitStatus {
                                how_cancelled: *this.how_cancelled,
                                status: None
                            }))
                        }
                        CancelSignal::Kill => {
                            // Kill the child process.
                            if let Some(id) = this.id {
                                unsafe {
                                    libc::kill(id as i32, libc::SIGKILL);
                                }
                            }
                            Poll::Ready(Ok(ExitStatus {
                                how_cancelled: *this.how_cancelled,
                                status: None
                            }))
                        }
                    },
                    // Keep waiting.
                    None => Poll::Pending
                }
            }
        }
    }
}

/// Future returned by [`CancellableChild::wait_with_output()`].  This future
/// will finish when the child process has exited or if the child process has
/// been cancelled, whichever comes first.
pub struct ChildWaitOutputFuture<F: FnMut() -> Option<CancelSignal> + Unpin, Fut: Future<Output = std::io::Result<std::process::Output>>> {
    id: Option<u32>,
    check_cancel: F,
    how_cancelled: Option<CancelSignal>,
    fut: Pin<Box<Fut>>,
}
impl<F: FnMut() -> Option<CancelSignal> + Unpin, Fut: Future<Output = std::io::Result<std::process::Output>>> Future for ChildWaitOutputFuture<F, Fut> {
    type Output = std::io::Result<Output>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // Need mutable self.
        let this = self.get_mut();

        // Check if the child process is being cancelled.
        let cancel_signal = (this.check_cancel)();

        // Poll the future.
        let poll_result = this.fut.as_mut().poll(cx);

        // Deal with result.
        match poll_result {
            // The child has finished.  Hooray!
            Poll::Ready(status) => match status {
                Ok(output) => {
                    Poll::Ready(Ok(Output {
                        how_cancelled: this.how_cancelled,
                        output: Some(output)
                    }))
                },
                Err(e) => Poll::Ready(Err(e))
            },
            // The child has not yet finished.
            Poll::Pending => {
                // Remember how we were cancelled.
                this.how_cancelled = cancel_signal;
                match cancel_signal {
                    // Cancel the child process and become ready immediately.
                    Some(cancel_signal) => match cancel_signal {
                        CancelSignal::Interrupt => {
                            // Interrupt the child process.
                            if let Some(id) = this.id {
                                unsafe {
                                    // Unsafe because we need to call libc, and
                                    // because process id may be stale.
                                    libc::kill(id as i32, libc::SIGINT);
                                }
                            }
                            Poll::Ready(Ok(Output {
                                how_cancelled: this.how_cancelled,
                                output: None
                            }))
                        }
                        CancelSignal::Kill => {
                            // Kill the child process.
                            if let Some(id) = this.id {
                                unsafe {
                                    libc::kill(id as i32, libc::SIGKILL);
                                }
                            }
                            Poll::Ready(Ok(Output {
                                how_cancelled: this.how_cancelled,
                                output: None
                            }))
                        }
                    },
                    // Keep waiting.
                    None => Poll::Pending
                }
            }
        }
    }
}

// TODO more exhaustive testing
#[cfg(test)]
mod tests {
    use super::*;
    use tokio::process::Command;

    #[tokio::test]
    async fn test_wait() {
        // Run the command `sleep 0.1` to completion.
        let child = Command::new("sleep").arg("0.1").spawn().unwrap();
        let mut child = CancellableChild::new(child, || None);
        let status = child.wait().await.unwrap();
        assert!(status.how_cancelled.is_none());
        assert!(status.status.unwrap().success());
    }

    #[tokio::test]
    async fn test_wait_cancel() {
        // Run the command `sleep 0.1` and then cancel it.
        let now = std::time::Instant::now();
        let child = Command::new("sleep").arg("0.1").spawn().unwrap();
        let mut child = CancellableChild::new(child, || Some(CancelSignal::Interrupt));
        let status = child.wait().await.unwrap();
        let elapsed = std::time::Instant::now().duration_since(now);
        assert!(elapsed < std::time::Duration::from_millis(100));
        assert!(status.how_cancelled.unwrap() == CancelSignal::Interrupt);
    }

    #[tokio::test]
    async fn test_wait_output() {
        // Run the command `echo hello` to completion.
        let child = Command::new("echo")
            .arg("hello")
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let child = CancellableChild::new(child, || None);
        let output = child.wait_with_output().await.unwrap();
        assert!(output.how_cancelled.is_none());
        let output = output.output.unwrap();
        assert!(output.status.success());
        assert!(std::str::from_utf8(&output.stdout).unwrap() == "hello\n");
    }
}
