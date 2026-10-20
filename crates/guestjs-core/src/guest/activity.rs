use std::cell::{
    Cell,
    RefCell,
};

use crate::{
    errors::Error,
    policy::{
        ArmedPolicy,
        ExecutionPolicy,
    },
};

pub struct GuestActivity {
    policy: ExecutionPolicy,
    depth: Cell<usize>,
    armed: RefCell<Option<ArmedPolicy>>,
}

impl GuestActivity {
    pub fn new(policy: ExecutionPolicy) -> Self {
        Self {
            policy,
            depth: Cell::default(),
            armed: RefCell::default(),
        }
    }

    fn finish(&self) {
        self.depth.set(self.depth.get() - 1);

        if self.depth.get() == 0 {
            *self.armed.borrow_mut() = None;
        }
    }

    pub fn policy(&self) -> &ExecutionPolicy {
        &self.policy
    }

    pub fn is_active(&self) -> bool {
        self.depth.get() > 0
    }

    pub fn begin(&self) -> Result<Activation<'_>, Error> {
        if !self.is_active() {
            let armed = self.policy.arm();

            if let Some(error) = armed.due_error() {
                return Err(error);
            }

            *self.armed.borrow_mut() = Some(armed);
        }

        self.depth.set(self.depth.get() + 1);

        Ok(Activation::new(self))
    }

    pub fn is_due(&self) -> bool {
        self.armed
            .borrow()
            .as_ref()
            .is_some_and(ArmedPolicy::is_due)
    }

    pub fn due_error(&self) -> Option<Error> {
        self.armed
            .borrow()
            .as_ref()
            .and_then(ArmedPolicy::due_error)
    }
}

pub struct Activation<'a> {
    activity: &'a GuestActivity,
}

impl<'a> Activation<'a> {
    fn new(activity: &'a GuestActivity) -> Self {
        Self {
            activity,
        }
    }
}

impl Drop for Activation<'_> {
    fn drop(&mut self) {
        self.activity.finish();
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::{
        guest::activity::GuestActivity,
        policy::{
            Cancellation,
            ExecutionPolicy,
        },
    };

    #[test]
    fn due_policy_refuses_the_outermost_activation() {
        let activity = GuestActivity::new(
            ExecutionPolicy::new().timeout(Duration::ZERO),
        );

        assert!(activity.begin().is_err_and(|error| error.is_timeout()));
        assert!(!activity.is_active());
        assert!(!activity.is_due());
    }

    #[test]
    fn outermost_activation_owns_the_armed_policy() {
        let cancellation = Cancellation::default();
        let activity = GuestActivity::new(
            ExecutionPolicy::new().cancellation(cancellation.clone()),
        );
        let outer = activity.begin().unwrap();

        drop(activity.begin().unwrap());

        assert!(activity.is_active());
        assert!(!activity.is_due());

        cancellation.cancel();

        assert!(activity.is_due());
        assert!(activity.due_error().is_some_and(|error| error.is_cancelled()));

        drop(outer);

        assert!(!activity.is_active());
        assert!(!activity.is_due());
        assert!(activity.due_error().is_none());
    }
}
