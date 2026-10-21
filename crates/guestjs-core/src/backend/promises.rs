use std::task::{self, Poll};

use crate::{
    backend::{Tok, Val, values::BackendValues},
    errors::Error,
};

pub enum PromiseState<V> {
    Pending,
    Fulfilled(V),
    Rejected(V),
}

pub trait BackendPromises: BackendValues {
    fn new_promise<'js>(
        token: Tok<'js, Self>,
    ) -> Result<(Val<'js, Self>, Val<'js, Self>, Val<'js, Self>), Error>;

    fn resolve<'js>(token: Tok<'js, Self>, value: Val<'js, Self>) -> Result<Val<'js, Self>, Error>;

    fn state<'js>(
        token: Tok<'js, Self>,
        promise: &Val<'js, Self>,
    ) -> Result<PromiseState<Val<'js, Self>>, Error>;

    fn poll_jobs(
        engine: &Self::Engine,
        context: &mut task::Context<'_>,
    ) -> Poll<Result<bool, Error>>;
}
