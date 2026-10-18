use crate::{
    backend::{
        values::BackendValues,
        Tok,
        Val,
    },
    errors::{
        Error,
        GuestErrorKind,
    },
};

pub trait BackendExceptions: BackendValues {
    fn throw<'js>(token: Tok<'js, Self>, value: Val<'js, Self>) -> Error;

    fn exception<'js>(
        token: Tok<'js, Self>,
        error: Error,
    ) -> Result<Val<'js, Self>, Error>;

    fn error<'js>(
        token: Tok<'js, Self>,
        kind: GuestErrorKind,
        message: &str,
    ) -> Result<Val<'js, Self>, Error>;
}
