use std::{
    any::{Any, type_name},
    marker::PhantomData,
    ops::Deref,
    rc::Rc,
};

use crate::{
    backend::Backend,
    errors::Error,
    host::{
        context::{CallContext, FromContext},
        requirements::{Requirement, Requirements},
    },
};

struct ModuleStateRequirement<S>(PhantomData<fn() -> S>);

impl<S: 'static> Requirement for ModuleStateRequirement<S> {
    fn check(&self, state: &HostState) -> Result<(), Error> {
        ModuleState::<S>::from_state(state).map(|_| ())
    }
}

#[derive(Clone)]
pub struct HostState {
    value: Option<Rc<dyn Any>>,
}

impl HostState {
    pub fn empty() -> Self {
        Self { value: None }
    }

    pub fn new<S: 'static>(state: S) -> Self {
        Self { value: Some(Rc::new(state)) }
    }

    pub fn get<S: 'static>(&self) -> Option<Rc<S>> {
        self.value.clone()?.downcast::<S>().ok()
    }
}

pub struct ModuleState<S> {
    value: Rc<S>,
}

impl<S> ModuleState<S> {
    pub fn new(value: Rc<S>) -> Self {
        Self { value }
    }
}

impl<S: 'static> ModuleState<S> {
    pub fn from_state(state: &HostState) -> Result<Self, Error> {
        state
            .get::<S>()
            .map(Self::new)
            .ok_or_else(|| {
                Error::unexpected(format!("host state does not contain {}", type_name::<S>(),))
            })
    }
}

impl<S> Clone for ModuleState<S> {
    fn clone(&self) -> Self {
        Self::new(self.value.clone())
    }
}

impl<S> Deref for ModuleState<S> {
    type Target = S;

    fn deref(&self) -> &S {
        self.value.as_ref()
    }
}

impl<B, S> FromContext<B> for ModuleState<S>
where
    B: Backend,
    S: 'static,
{
    fn from_context<'e, 'js>(context: &CallContext<'e, 'js, B>) -> Result<Self, Error> {
        Self::from_state(context.state())
    }

    fn declare(requirements: &mut Requirements) {
        requirements.push(ModuleStateRequirement::<S>(PhantomData));
    }
}

#[cfg(test)]
mod tests {
    use std::{ptr, rc::Rc};

    use crate::host::state::{HostState, ModuleState};

    #[test]
    fn empty_state_holds_nothing() {
        assert!(HostState::empty().get::<u8>().is_none());
    }

    #[test]
    fn state_is_read_as_its_own_type() {
        assert_eq!(
            *HostState::new(7_u8)
                .get::<u8>()
                .unwrap(),
            7
        );
    }

    #[test]
    fn state_is_not_read_as_another_type() {
        assert!(
            HostState::new(7_u8)
                .get::<u16>()
                .is_none()
        );
    }

    #[test]
    fn clones_share_one_state() {
        let state = HostState::new(7_u8);

        assert!(Rc::ptr_eq(&state.get::<u8>().unwrap(), &state.clone().get::<u8>().unwrap(),));
    }

    #[test]
    fn typed_state_uses_the_existing_allocation() {
        let state = HostState::new(7_u8);
        let resolved = ModuleState::<u8>::from_state(&state).unwrap();

        assert_eq!(*resolved, 7);
        assert!(ptr::eq(&*resolved, state.get::<u8>().unwrap().as_ref(),));
    }

    #[test]
    fn clones_share_state_without_requiring_a_cloneable_payload() {
        struct State {
            _value: u8,
        }

        let resolved =
            ModuleState::<State>::from_state(&HostState::new(State { _value: 7 })).unwrap();

        assert!(ptr::eq(&*resolved, &*resolved.clone()));
    }

    #[test]
    fn empty_state_is_an_error() {
        assert!(ModuleState::<u8>::from_state(&HostState::empty()).is_err());
    }

    #[test]
    fn another_state_type_is_an_error() {
        assert!(ModuleState::<u8>::from_state(&HostState::new(7_u16)).is_err());
    }
}
