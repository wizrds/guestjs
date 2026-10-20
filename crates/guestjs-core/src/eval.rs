use crate::{
    backend::{modules::BackendModules, values::BackendValues},
    errors::Error,
    guest::Guest,
    handle::object::Object,
    marshal::FromGuest,
};

pub trait GuestEvalExt<B>
where
    B: BackendModules,
{
    fn exec(&self, source: &str) -> Result<(), Error>;

    fn eval<T>(&self, source: &str) -> Result<T::Owned, Error>
    where
        T: FromGuest<B>;
}

impl<B> GuestEvalExt<B> for Guest<B>
where
    B: BackendModules,
{
    fn exec(&self, source: &str) -> Result<(), Error> {
        self.enter(|enter| B::eval(enter.token(), source, "<guest>").map(drop))
    }

    fn eval<T>(&self, source: &str) -> Result<T::Owned, Error>
    where
        T: FromGuest<B>,
    {
        self.enter(|enter| T::from_guest(enter, B::eval(enter.token(), source, "<guest>")?))
    }
}

pub trait GuestGlobalsExt<B>
where
    B: BackendValues,
{
    fn globals(&self) -> Result<Object<B>, Error>;
}

impl<B> GuestGlobalsExt<B> for Guest<B>
where
    B: BackendValues,
{
    fn globals(&self) -> Result<Object<B>, Error> {
        self.enter(|enter| Object::<B>::from_guest(enter, B::globals(enter.token())))
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        backend::tests::Stub,
        eval::{GuestEvalExt, GuestGlobalsExt},
        guest::Guest,
        runtime::Runtime,
    };

    #[test]
    fn closed_guests_refuse_scripts_and_globals() {
        let runtime = Runtime::<Stub>::new(()).unwrap();
        let guest = Guest::new(&runtime).unwrap();

        guest.close();

        assert!(
            guest
                .exec("")
                .is_err_and(|error| error.is_closed())
        );
        assert!(
            guest
                .eval::<f64>("")
                .is_err_and(|error| error.is_closed())
        );
        assert!(
            guest
                .globals()
                .is_err_and(|error| error.is_closed())
        );
    }
}
