use std::rc::Rc;

use crate::{errors::Error, host::state::HostState};

pub trait Requirement {
    fn check(&self, state: &HostState) -> Result<(), Error>;
}

#[derive(Clone, Default)]
pub struct Requirements {
    entries: Vec<Rc<dyn Requirement>>,
}

impl Requirements {
    pub fn new() -> Self {
        Self { entries: Vec::new() }
    }

    pub fn push<R>(&mut self, requirement: R)
    where
        R: Requirement + 'static,
    {
        self.entries.push(Rc::new(requirement));
    }

    pub fn check(&self, state: &HostState) -> Result<(), Error> {
        self.entries
            .iter()
            .try_for_each(|requirement| requirement.check(state))
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use crate::{
        errors::Error,
        host::{
            requirements::{Requirement, Requirements},
            state::HostState,
        },
    };

    struct RecordingRequirement {
        order: Rc<RefCell<Vec<u8>>>,
        value: u8,
        fails: bool,
    }

    impl RecordingRequirement {
        fn new(order: Rc<RefCell<Vec<u8>>>, value: u8, fails: bool) -> Self {
            Self { order, value, fails }
        }
    }

    impl Requirement for RecordingRequirement {
        fn check(&self, _state: &HostState) -> Result<(), Error> {
            self.order.borrow_mut().push(self.value);

            if self.fails {
                Err(Error::unexpected("requirement failed"))
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn empty_requirements_succeed() {
        assert!(
            Requirements::new()
                .check(&HostState::empty())
                .is_ok()
        );
        assert!(
            Requirements::default()
                .check(&HostState::empty())
                .is_ok()
        );
    }

    #[test]
    fn checks_follow_registration_order() {
        let order = Rc::new(RefCell::new(Vec::new()));
        let mut requirements = Requirements::new();

        requirements.push(RecordingRequirement::new(order.clone(), 1, false));
        requirements.push(RecordingRequirement::new(order.clone(), 2, false));

        requirements
            .check(&HostState::empty())
            .unwrap();

        assert_eq!(*order.borrow(), [1, 2]);
    }

    #[test]
    fn first_failure_stops_later_checks() {
        let order = Rc::new(RefCell::new(Vec::new()));
        let mut requirements = Requirements::new();

        requirements.push(RecordingRequirement::new(order.clone(), 1, true));
        requirements.push(RecordingRequirement::new(order.clone(), 2, false));

        assert!(
            requirements
                .check(&HostState::empty())
                .is_err()
        );
        assert_eq!(*order.borrow(), [1]);
    }

    #[test]
    fn cloning_does_not_share_registration_changes() {
        let order = Rc::new(RefCell::new(Vec::new()));
        let mut requirements = Requirements::new();

        requirements.push(RecordingRequirement::new(order.clone(), 1, false));

        let mut cloned = requirements.clone();

        cloned.push(RecordingRequirement::new(order.clone(), 2, false));

        requirements
            .check(&HostState::empty())
            .unwrap();

        assert_eq!(*order.borrow(), [1]);

        order.borrow_mut().clear();

        cloned
            .check(&HostState::empty())
            .unwrap();

        assert_eq!(*order.borrow(), [1, 2]);
    }
}
