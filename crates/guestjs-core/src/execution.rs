use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use crate::errors::Error;

/// A synchronous guest-execution cancellation signal.
pub trait CancelSignal: Send + Sync + 'static {
    /// Returns whether cancellation was requested.
    fn is_cancelled(&self) -> bool;
}

/// A shared guest-execution cancellation signal.
#[derive(Clone, Default)]
pub struct Cancellation(Arc<AtomicBool>);

impl Cancellation {
    /// Creates an uncancelled [`Cancellation`](crate::execution::Cancellation).
    pub fn new() -> Self {
        Self::default()
    }

    /// Requests cancellation.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    /// Returns whether cancellation was requested.
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

impl CancelSignal for Cancellation {
    fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

#[cfg(feature = "tokio")]
impl CancelSignal for tokio_util::sync::CancellationToken {
    fn is_cancelled(&self) -> bool {
        tokio_util::sync::CancellationToken::is_cancelled(self)
    }
}

#[derive(Clone)]
struct Deadline {
    base: Instant,
    at: Arc<AtomicU64>,
}

impl Deadline {
    fn arm(&self, budget: Duration) {
        self.at.store(
            (self.base.elapsed() + budget).as_nanos() as u64,
            Ordering::Relaxed,
        );
    }

    fn disarm(&self) {
        self.at.store(0, Ordering::Relaxed);
    }

    fn expired(&self) -> bool {
        match self.at.load(Ordering::Relaxed) {
            0 => false,
            at => self.base.elapsed().as_nanos() as u64 >= at,
        }
    }
}

impl Default for Deadline {
    fn default() -> Self {
        Self {
            base: Instant::now(),
            at: Arc::new(AtomicU64::new(0)),
        }
    }
}

#[derive(Clone, Default)]
pub(crate) struct ExecutionPolicy {
    timeout: Option<Duration>,
    cancellation: Option<Arc<dyn CancelSignal>>,
    deadline: Deadline,
    gc_after: Option<u32>,
    since_gc: Arc<AtomicU32>,
}

impl ExecutionPolicy {
    pub(crate) fn new(
        timeout: Option<Duration>,
        cancellation: Option<Arc<dyn CancelSignal>>,
        gc_after: Option<u32>,
    ) -> Self {
        Self {
            timeout,
            cancellation,
            deadline: Deadline::default(),
            gc_after,
            since_gc: Arc::new(AtomicU32::new(0)),
        }
    }

    fn is_cancelled(&self) -> bool {
        self.cancellation
            .as_ref()
            .is_some_and(|signal| signal.is_cancelled())
    }

    pub(crate) fn begin(&self) -> Result<(), Error> {
        if self.is_cancelled() {
            return Err(Error::cancelled());
        }

        if let Some(budget) = self.timeout {
            self.deadline.arm(budget);
        }

        Ok(())
    }

    pub(crate) fn should_abort(&self) -> bool {
        self.is_cancelled() || self.deadline.expired()
    }

    pub(crate) fn classify<R>(&self, result: Result<R, Error>) -> Result<R, Error> {
        match result {
            Err(error) if error.is_interrupt() && self.is_cancelled() => {
                Err(Error::cancelled())
            }
            Err(error) if error.is_interrupt() && self.deadline.expired() => {
                Err(Error::timeout())
            }
            other => other,
        }
    }

    pub(crate) fn disarm(&self) {
        self.deadline.disarm();
    }

    pub(crate) fn should_gc(&self) -> bool {
        let Some(limit) = self.gc_after else {
            return false;
        };

        if self.since_gc.fetch_add(1, Ordering::Relaxed) + 1 < limit {
            return false;
        }

        self.since_gc.store(0, Ordering::Relaxed);

        true
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, thread::sleep, time::Duration};

    use crate::{
        errors::Error,
        execution::{Cancellation, ExecutionPolicy},
    };

    #[test]
    fn a_cancelled_policy_is_rejected_at_begin() {
        let cancellation = Cancellation::new();

        cancellation.cancel();

        assert!(matches!(
            ExecutionPolicy::new(
                None,
                Some(Arc::new(cancellation)),
                None,
            )
            .begin(),
            Err(Error::Cancelled),
        ));
    }

    #[test]
    fn an_expired_deadline_refines_an_interrupt_into_a_timeout() {
        let policy = ExecutionPolicy::new(
            Some(Duration::from_millis(1)),
            None,
            None,
        );

        policy.begin().unwrap();

        sleep(Duration::from_millis(5));

        assert!(matches!(
            policy.classify::<()>(Err(Error::interrupted())),
            Err(Error::Timeout),
        ));
    }

    #[test]
    fn a_cancellation_refines_an_interrupt_into_a_cancellation() {
        let cancellation = Cancellation::new();
        let policy = ExecutionPolicy::new(
            None,
            Some(Arc::new(cancellation.clone())),
            None,
        );

        policy.begin().unwrap();

        cancellation.cancel();

        assert!(matches!(
            policy.classify::<()>(Err(Error::interrupted())),
            Err(Error::Cancelled),
        ));
    }

    #[test]
    fn a_non_interrupt_error_passes_through_even_past_the_deadline() {
        let policy = ExecutionPolicy::new(
            Some(Duration::from_millis(1)),
            None,
            None,
        );

        policy.begin().unwrap();

        sleep(Duration::from_millis(5));

        assert!(matches!(
            policy.classify::<()>(Err(Error::guest_exception("thrown"))),
            Err(Error::GuestException { .. }),
        ));
    }

    #[test]
    fn a_healthy_result_passes_through_unchanged() {
        let policy = ExecutionPolicy::new(
            Some(Duration::from_secs(10)),
            None,
            None,
        );

        policy.begin().unwrap();

        assert_eq!(policy.classify(Ok::<_, Error>(7)).unwrap(), 7);
    }

    #[test]
    fn should_gc_fires_every_nth_execution() {
        let policy = ExecutionPolicy::new(None, None, Some(3));

        assert_eq!(
            (0..6)
                .map(|_| policy.should_gc())
                .collect::<Vec<_>>(),
            vec![false, false, true, false, false, true],
        );
    }
}
