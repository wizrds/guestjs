use std::{
    sync::{
        atomic::{
            AtomicBool,
            Ordering,
        },
        Arc,
    },
    time::{
        Duration,
        Instant,
    },
};

use crate::errors::Error;

#[derive(Clone, Default, Debug)]
pub struct Cancellation {
    cancelled: Arc<AtomicBool>,
}

impl Cancellation {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }
}

#[derive(Clone, Default, Debug)]
pub struct ExecutionPolicy {
    pub timeout: Option<Duration>,
    pub cancellation: Option<Cancellation>,
}

#[derive(Debug)]
pub struct ArmedPolicy {
    deadline: Option<Instant>,
    cancellation: Option<Cancellation>,
}

impl ArmedPolicy {
    pub fn new(policy: &ExecutionPolicy) -> Self {
        Self {
            deadline: policy.timeout.and_then(|timeout| {
                Instant::now().checked_add(timeout)
            }),
            cancellation: policy.cancellation.clone(),
        }
    }

    pub fn is_expired(&self) -> bool {
        self.deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancellation
            .as_ref()
            .is_some_and(Cancellation::is_cancelled)
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
    use std::{
        thread,
        time::Duration,
    };

    use crate::policy::{
        ArmedPolicy,
        Cancellation,
        ExecutionPolicy,
    };

    #[test]
    fn default_policy_is_never_due() {
        assert!(!ArmedPolicy::new(&ExecutionPolicy::default()).is_due());
    }

    #[test]
    fn zero_timeout_is_due_with_timeout() {
        let armed = ArmedPolicy::new(&ExecutionPolicy {
            timeout: Some(Duration::ZERO),
            cancellation: None,
        });

        assert!(armed.is_due());
        assert!(armed.due_error().is_some_and(|error| error.is_timeout()));
    }

    #[test]
    fn unrepresentable_deadline_never_expires() {
        assert!(!ArmedPolicy::new(&ExecutionPolicy {
            timeout: Some(Duration::MAX),
            cancellation: None,
        })
        .is_expired());
    }

    #[test]
    fn cancellation_crosses_threads() {
        let cancellation = Cancellation::default();
        let armed = ArmedPolicy::new(&ExecutionPolicy {
            timeout: None,
            cancellation: Some(cancellation.clone()),
        });

        assert!(!armed.is_due());

        thread::spawn(move || cancellation.cancel())
            .join()
            .unwrap();

        assert!(armed.due_error().is_some_and(|error| error.is_cancelled()));
    }

    #[test]
    fn expired_deadline_takes_precedence_over_cancellation() {
        let cancellation = Cancellation::default();

        cancellation.cancel();

        assert!(
            ArmedPolicy::new(&ExecutionPolicy {
                timeout: Some(Duration::ZERO),
                cancellation: Some(cancellation),
            })
            .due_error()
            .is_some_and(|error| error.is_timeout()),
        );
    }
}
