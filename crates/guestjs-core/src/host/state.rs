use std::{
    any::Any,
    rc::Rc,
};

#[derive(Clone)]
pub struct HostState {
    value: Option<Rc<dyn Any>>,
}

impl HostState {
    pub fn empty() -> Self {
        Self {
            value: None,
        }
    }

    pub fn new<S: 'static>(state: S) -> Self {
        Self {
            value: Some(Rc::new(state)),
        }
    }

    pub fn get<S: 'static>(&self) -> Option<Rc<S>> {
        self.value
            .clone()?
            .downcast::<S>()
            .ok()
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use crate::host::state::HostState;

    #[test]
    fn empty_state_holds_nothing() {
        assert!(HostState::empty().get::<u8>().is_none());
    }

    #[test]
    fn state_is_read_as_its_own_type() {
        assert_eq!(*HostState::new(7_u8).get::<u8>().unwrap(), 7);
    }

    #[test]
    fn state_is_not_read_as_another_type() {
        assert!(HostState::new(7_u8).get::<u16>().is_none());
    }

    #[test]
    fn clones_share_one_state() {
        let state = HostState::new(7_u8);

        assert!(Rc::ptr_eq(
            &state.get::<u8>().unwrap(),
            &state.clone().get::<u8>().unwrap(),
        ));
    }
}
