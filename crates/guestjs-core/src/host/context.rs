use crate::{
    backend::Backend,
    enter::Enter,
    errors::Error,
    host::{requirements::Requirements, state::HostState},
};

pub trait FromContext<B: Backend>: Sized {
    fn from_context<'e, 'js>(context: &CallContext<'e, 'js, B>) -> Result<Self, Error>;

    fn declare(_requirements: &mut Requirements) {}
}

pub struct CallContext<'e, 'js, B: Backend> {
    enter: &'e Enter<'js, B>,
    state: &'e HostState,
}

impl<'e, 'js, B: Backend> CallContext<'e, 'js, B> {
    pub fn new(enter: &'e Enter<'js, B>, state: &'e HostState) -> Self {
        Self { enter, state }
    }

    pub fn enter(&self) -> &'e Enter<'js, B> {
        self.enter
    }

    pub fn state(&self) -> &'e HostState {
        self.state
    }

    pub fn resolve<T>(&self) -> Result<T, Error>
    where
        T: FromContext<B>,
    {
        T::from_context(self)
    }
}
