use std::{
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use crate::errors::Error;

pub trait CancelSignal: 'static {
    fn is_cancelled(&self) -> bool;
}

#[derive(Clone, Debug)]
pub struct Cancellation {
    cancelled: Arc<AtomicBool>,
}

impl Cancellation {
    pub fn new() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn cancel(&self) {
        self.cancelled
            .store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}

impl Default for Cancellation {
    fn default() -> Self {
        Self::new()
    }
}

impl CancelSignal for Cancellation {
    fn is_cancelled(&self) -> bool {
        Cancellation::is_cancelled(self)
    }
}

#[cfg(feature = "tokio")]
impl CancelSignal for tokio_util::sync::CancellationToken {
    fn is_cancelled(&self) -> bool {
        tokio_util::sync::CancellationToken::is_cancelled(self)
    }
}

#[derive(Clone)]
pub struct ExecutionPolicy {
    timeout: Option<Duration>,
    cancellation: Option<Rc<dyn CancelSignal>>,
}

impl ExecutionPolicy {
    pub fn new() -> Self {
        Self { timeout: None, cancellation: None }
    }

    pub fn timeout<T>(mut self, timeout: T) -> Self
    where
        T: Into<Duration>,
    {
        self.timeout = Some(timeout.into());

        self
    }

    pub fn cancellation<S>(mut self, signal: S) -> Self
    where
        S: CancelSignal,
    {
        self.cancellation = Some(Rc::new(signal));

        self
    }

    pub fn arm(&self) -> ArmedPolicy {
        ArmedPolicy::new(self.timeout, self.cancellation.clone())
    }
}

impl Default for ExecutionPolicy {
    fn default() -> Self {
        Self::new()
    }
}

pub struct ArmedPolicy {
    deadline: Option<Instant>,
    cancellation: Option<Rc<dyn CancelSignal>>,
}

impl ArmedPolicy {
    pub fn new(timeout: Option<Duration>, cancellation: Option<Rc<dyn CancelSignal>>) -> Self {
        Self {
            deadline: timeout.and_then(|timeout| Instant::now().checked_add(timeout)),
            cancellation,
        }
    }

    pub fn is_expired(&self) -> bool {
        self.deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancellation
            .as_ref()
            .is_some_and(|signal| signal.is_cancelled())
    }

    pub fn due_error(&self) -> Option<Error> {
        match (self.is_expired(), self.is_cancelled()) {
            (true, _) => Some(Error::timeout()),
            (false, true) => Some(Error::cancelled()),
            (false, false) => None,
        }
    }

    pub fn is_due(&self) -> bool {
        self.is_expired() || self.is_cancelled()
    }
}

#[cfg(test)]
mod tests {
    use std::{thread, time::Duration};

    use crate::policy::{CancelSignal, Cancellation, ExecutionPolicy};

    #[test]
    fn default_policy_is_never_due() {
        assert!(
            !ExecutionPolicy::default()
                .arm()
                .is_due()
        );
    }

    #[test]
    fn zero_timeout_is_due_with_timeout() {
        let armed = ExecutionPolicy::new()
            .timeout(Duration::ZERO)
            .arm();

        assert!(armed.is_due());
        assert!(
            armed
                .due_error()
                .is_some_and(|error| error.is_timeout())
        );
    }

    #[test]
    fn unrepresentable_deadline_never_expires() {
        assert!(
            !ExecutionPolicy::new()
                .timeout(Duration::MAX)
                .arm()
                .is_expired()
        );
    }

    #[test]
    fn cancellation_crosses_threads() {
        let cancellation = Cancellation::new();
        let armed = ExecutionPolicy::new()
            .cancellation(cancellation.clone())
            .arm();

        assert!(!armed.is_due());

        thread::spawn(move || cancellation.cancel())
            .join()
            .unwrap();

        assert!(
            armed
                .due_error()
                .is_some_and(|error| error.is_cancelled())
        );
    }

    #[test]
    fn expired_deadline_takes_precedence_over_cancellation() {
        let cancellation = Cancellation::new();

        cancellation.cancel();

        assert!(
            ExecutionPolicy::new()
                .timeout(Duration::ZERO)
                .cancellation(cancellation)
                .arm()
                .due_error()
                .is_some_and(|error| error.is_timeout()),
        );
    }

    #[test]
    fn custom_signals_cancel() {
        struct Cancelled;

        impl CancelSignal for Cancelled {
            fn is_cancelled(&self) -> bool {
                true
            }
        }

        assert!(
            ExecutionPolicy::new()
                .cancellation(Cancelled)
                .arm()
                .due_error()
                .is_some_and(|error| error.is_cancelled())
        );
    }
}
