use crate::{
    backend::{
        Backend,
        Val,
    },
    enter::Enter,
    errors::Error,
    guest::Guest,
};

pub struct Handle<B: Backend> {
    owned: B::Owned,
    guest: Guest<B>,
}

impl<B: Backend> Handle<B> {
    pub fn from_value<'js>(
        enter: &Enter<'js, B>,
        value: Val<'js, B>,
    ) -> Self {
        Self {
            owned: B::detach(enter.token(), value),
            guest: enter.guest().clone(),
        }
    }

    pub fn guest(&self) -> &Guest<B> {
        &self.guest
    }

    pub fn same(&self, other: &Self) -> bool {
        self.guest.id() == other.guest.id()
            && B::owned_ptr_eq(&self.owned, &other.owned)
    }

    pub fn attach<'js>(
        &self,
        enter: &Enter<'js, B>,
    ) -> Result<Val<'js, B>, Error> {
        B::attach(enter.token(), &self.owned)
    }

    pub fn with_enter<F, R>(&self, f: F) -> Result<R, Error>
    where
        F: for<'js> FnOnce(
            &Enter<'js, B>,
            &Val<'js, B>,
        ) -> Result<R, Error>,
    {
        self.guest
            .enter(|enter| f(enter, &self.attach(enter)?))
    }
}

impl<B: Backend> Clone for Handle<B> {
    fn clone(&self) -> Self {
        Self {
            owned: self.owned.clone(),
            guest: self.guest.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use crate::{
        backend::{
            tests::Stub,
            Backend,
        },
        errors::Error,
        guest::Guest,
        handle::base::Handle,
        runtime::Runtime,
    };

    #[test]
    fn with_enter_uses_the_owning_guest() {
        let guest = Guest::new(&Runtime::<Stub>::new(()).unwrap()).unwrap();
        let handle = guest
            .enter(|enter| Ok(Handle::from_value(enter, ())))
            .unwrap();

        assert_eq!(handle.guest().id(), guest.id());
        assert_eq!(
            handle
                .with_enter(|enter, _| Ok(Stub::context_id(enter.token())))
                .unwrap(),
            guest.context_id(),
        );
    }

    #[test]
    fn a_clone_retains_the_guest_after_other_owners_drop() {
        let guest = Guest::new(&Runtime::<Stub>::new(()).unwrap()).unwrap();
        let handle = guest
            .enter(|enter| Ok(Handle::from_value(enter, ())))
            .unwrap();
        let cloned = handle.clone();

        assert!(cloned.same(&handle));

        drop(handle);
        drop(guest);

        assert!(cloned.with_enter(|_, _| Ok(())).is_ok());
    }

    #[test]
    fn handles_from_different_guests_are_not_the_same() {
        let runtime = Runtime::<Stub>::new(()).unwrap();
        let first = Guest::new(&runtime)
            .unwrap()
            .enter(|enter| Ok(Handle::from_value(enter, ())))
            .unwrap();

        assert!(!first.same(
            &Guest::new(&runtime)
                .unwrap()
                .enter(|enter| Ok(Handle::from_value(enter, ())))
                .unwrap(),
        ));
    }

    #[test]
    fn a_closed_guest_does_not_invoke_the_operation() {
        let handle = Guest::new(&Runtime::<Stub>::new(()).unwrap())
            .unwrap()
            .enter(|enter| Ok(Handle::from_value(enter, ())))
            .unwrap();
        let called = Cell::new(false);

        handle.guest().close();

        assert!(handle
            .with_enter(|_, _| {
                called.set(true);

                Ok(())
            })
            .is_err_and(|error| error.is_closed()));
        assert!(!called.get());
    }

    #[test]
    fn operation_errors_are_preserved() {
        assert!(Guest::new(&Runtime::<Stub>::new(()).unwrap())
            .unwrap()
            .enter(|enter| Ok(Handle::from_value(enter, ())))
            .unwrap()
            .with_enter(|_, _| {
                Err::<(), _>(Error::conversion("number", "string"))
            })
            .is_err_and(|error| error.is_conversion()));
    }
}
