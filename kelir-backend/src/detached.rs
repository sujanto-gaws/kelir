//! The work a request hands to the runtime and does not wait for ([#856]).
//!
//! Today that is the reset mail ([`crate::mail::Mailer::send_detached`]). It is
//! spawned here rather than with a bare `tokio::spawn` so a stop can do two
//! things with it that a loose task does not allow: **wait for it**, and, at the
//! stop's deadline, **abort what is still running**.
//!
//! A `TaskTracker` alone gives the first and not the second: closing it and
//! timing out its `wait` leaves the tracked tasks running. So each task here
//! also races a [`CancellationToken`], and [`DetachedTasks::abandon`] cancels
//! it: every task still running ends at its next poll, its work dropped.
//!
//! [#856]: https://github.com/sujanto-gaws/kelir/issues/856

use std::future::Future;

use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

/// Detached tasks a stop can wait for and abandon. Cloning shares them.
#[derive(Clone, Default)]
pub struct DetachedTasks {
    tracker: TaskTracker,
    abandoned: CancellationToken,
}

impl DetachedTasks {
    pub fn new() -> Self {
        Self::default()
    }

    /// Runs `task` beside the request that hands it off, until it finishes or
    /// [`Self::abandon`] is called.
    pub fn spawn<F>(&self, task: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let abandoned = self.abandoned.clone();

        self.tracker.spawn(async move {
            tokio::select! {
                () = task => {}
                () = abandoned.cancelled() => {}
            }
        });
    }

    /// Says no more tasks are expected, so [`Self::wait`] can complete once
    /// the running ones end. A task spawned after still runs.
    pub fn close(&self) {
        self.tracker.close();
    }

    /// Completes once [`Self::close`] has been called and no task is running.
    pub async fn wait(&self) {
        self.tracker.wait().await;
    }

    /// Ends every task still running, at its next poll, and every task
    /// spawned from now on before it starts.
    pub fn abandon(&self) {
        self.abandoned.cancel();
    }

    /// How many tasks are still running.
    pub fn len(&self) -> usize {
        self.tracker.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tracker.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[tokio::test]
    async fn a_task_that_ends_is_waited_for() {
        let tasks = DetachedTasks::new();
        tasks.spawn(async {});
        tasks.close();

        tokio::time::timeout(Duration::from_secs(1), tasks.wait())
            .await
            .expect("the finished task is waited for");
        assert!(tasks.is_empty());
    }

    #[tokio::test]
    async fn an_abandoned_task_ends_without_finishing() {
        let tasks = DetachedTasks::new();
        let (finished, mut was_finished) = tokio::sync::oneshot::channel::<()>();
        tasks.spawn(async move {
            tokio::time::sleep(Duration::from_secs(3600)).await;
            let _ = finished.send(());
        });
        tasks.close();
        assert_eq!(tasks.len(), 1, "control: the task is running");

        tasks.abandon();

        tokio::time::timeout(Duration::from_secs(1), tasks.wait())
            .await
            .expect("the abandoned task ends");
        assert!(tasks.is_empty());
        assert!(
            was_finished.try_recv().is_err(),
            "the abandoned task ran to its end"
        );
    }
}
