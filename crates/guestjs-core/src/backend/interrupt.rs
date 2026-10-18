use std::rc::Rc;

use crate::{
    backend::{
        Backend,
        Tok,
    },
    errors::Error,
};

pub type InterruptCondition = Rc<dyn Fn() -> bool>;

pub trait BackendInterrupt: Backend {
    fn set_interrupt_condition(
        engine: &Self::Engine,
        condition: InterruptCondition,
    );

    fn check<'js>(token: Tok<'js, Self>) -> Result<(), Error>;
}
