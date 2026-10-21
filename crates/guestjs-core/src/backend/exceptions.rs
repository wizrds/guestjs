use crate::{
    backend::{Tok, Val, values::BackendValues},
    errors::{Error, GuestErrorKind},
};

pub trait BackendExceptions: BackendValues {
    fn throw<'js>(token: Tok<'js, Self>, value: Val<'js, Self>) -> Error;

    fn new_exception<'js>(token: Tok<'js, Self>, error: Error) -> Result<Val<'js, Self>, Error>;

    fn new_error<'js>(
        token: Tok<'js, Self>,
        kind: GuestErrorKind,
        message: &str,
    ) -> Result<Val<'js, Self>, Error>;
}
