use std::rc::Rc;

use crate::{
    backend::{
        values::BackendValues,
        Backend,
        Tok,
        Val,
    },
    errors::Error,
};

pub struct RawCall<'js, B: Backend> {
    pub token: Tok<'js, B>,
    pub this: Val<'js, B>,
    pub args: Vec<Val<'js, B>>,
    pub new_target: Option<Val<'js, B>>,
}

pub type RawBody<B> = Rc<
    dyn for<'js> Fn(RawCall<'js, B>) -> Result<Val<'js, B>, Error>,
>;

pub trait BackendCallables: BackendValues {
    fn function<'js>(
        token: Tok<'js, Self>,
        name: &str,
        length: u32,
        body: RawBody<Self>,
    ) -> Result<Val<'js, Self>, Error>;
}
