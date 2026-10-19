use std::marker::PhantomData;

use crate::{
    backend::{
        classes::BackendClasses,
        exceptions::BackendExceptions,
        values::Property,
        Val,
    },
    enter::Enter,
    errors::{
        Error,
        GuestErrorKind,
    },
    host::{
        class::{
            ClassPrototype,
            RealisedClass,
            RealisedClasses,
        },
        declaration::Declaration,
        state::HostState,
    },
    marshal::{
        FromGuest,
        ToGuest,
    },
};

pub trait HostException: Sized + 'static {
    const NAME: &'static str;
    const BASE: GuestErrorKind = GuestErrorKind::Error;

    fn throw<'js, B>(enter: &Enter<'js, B>, message: &str) -> Error
    where
        B: BackendClasses + BackendExceptions,
    {
        let class = ExceptionDeclaration::<Self>::new();

        class
            .realised(enter)
            .and_then(|realised| {
                class.instantiate(enter, &realised.prototype().get(enter)?, message)
            })
            .map(|error| B::throw(enter.token(), error))
            .unwrap_or_else(|error| error)
    }

    fn caught(error: &Error) -> bool {
        matches!(error, Error::Guest(exception) if exception.matches(Self::NAME))
    }
}

pub struct ExceptionDeclaration<E: HostException> {
    exception: PhantomData<fn() -> E>,
}

impl<E: HostException> ExceptionDeclaration<E> {
    pub fn new() -> Self {
        Self {
            exception: PhantomData,
        }
    }

    pub fn realised<'js, B>(&self, enter: &Enter<'js, B>) -> Result<RealisedClass<B>, Error>
    where
        B: BackendClasses + BackendExceptions,
    {
        RealisedClasses::of(enter.guest()).get_or_realise::<E, _>(|| {
            let prototype = B::object(enter.token())?;

            B::set_prototype(
                enter.token(),
                &prototype,
                &B::prototype_of(enter.token(), &B::error(enter.token(), E::BASE, "")?)?,
            )?;

            B::define(
                enter.token(),
                &prototype,
                &"name".to_guest(enter)?,
                Property::Value {
                    value: E::NAME.to_guest(enter)?,
                    writable: true,
                    enumerable: false,
                    configurable: true,
                },
            )?;

            RealisedClass::link(
                enter,
                B::constructor(
                    enter.token(),
                    E::NAME,
                    1,
                    enter.guest().raw_body({
                        let prototype = ClassPrototype::new(enter, prototype.clone());

                        move |enter, call| {
                            ExceptionDeclaration::<E>::new().instantiate(
                                enter,
                                &prototype.for_target(enter, call.new_target.as_ref())?,
                                &call
                                    .args
                                    .first()
                                    .map(|message| {
                                        Option::<String>::from_guest(enter, message.clone())
                                    })
                                    .transpose()?
                                    .flatten()
                                    .unwrap_or_default(),
                            )
                        }
                    }),
                )?,
                prototype,
            )
        })
    }

    pub fn instantiate<'js, B>(
        &self,
        enter: &Enter<'js, B>,
        prototype: &Val<'js, B>,
        message: &str,
    ) -> Result<Val<'js, B>, Error>
    where
        B: BackendExceptions,
    {
        let error = B::error(enter.token(), E::BASE, message)?;

        B::set_prototype(enter.token(), &error, prototype)?;

        Ok(error)
    }
}

impl<E: HostException> Default for ExceptionDeclaration<E> {
    fn default() -> Self {
        Self::new()
    }
}

impl<B, E> Declaration<B> for ExceptionDeclaration<E>
where
    B: BackendClasses + BackendExceptions,
    E: HostException,
{
    fn realise<'js>(
        &self,
        enter: &Enter<'js, B>,
        _state: &HostState,
        _name: &str,
    ) -> Result<Val<'js, B>, Error> {
        self.realised(enter)?
            .constructor(enter)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        backend::{
            classes::BackendClasses,
            exceptions::BackendExceptions,
        },
        enter::Enter,
        errors::{
            Error,
            GuestException,
        },
        host::{
            declaration::Declaration,
            exception::{
                ExceptionDeclaration,
                HostException,
            },
        },
    };

    struct NetworkError;

    impl HostException for NetworkError {
        const NAME: &'static str = "NetworkError";
    }

    #[test]
    fn a_thrown_exception_is_caught_by_its_name() {
        assert!(NetworkError::caught(&Error::guest(GuestException::new(
            "NetworkError".to_owned(),
            "offline".to_owned(),
            None,
            vec!["NetworkError".to_owned(), "Error".to_owned()],
            (),
        ))));
    }

    #[test]
    fn a_guest_subclass_is_caught_by_its_base_name() {
        assert!(NetworkError::caught(&Error::guest(GuestException::new(
            "TimeoutError".to_owned(),
            "timed out".to_owned(),
            None,
            vec![
                "TimeoutError".to_owned(),
                "NetworkError".to_owned(),
                "Error".to_owned(),
            ],
            (),
        ))));
    }

    #[test]
    fn another_exception_is_not_caught() {
        assert!(!NetworkError::caught(&Error::guest(GuestException::new(
            "TypeError".to_owned(),
            "bad".to_owned(),
            None,
            vec!["TypeError".to_owned(), "Error".to_owned()],
            (),
        ))));
    }

    #[test]
    fn a_host_error_is_not_caught() {
        assert!(!NetworkError::caught(&Error::closed()));
    }

    #[allow(dead_code)]
    fn capabilities<B: BackendClasses + BackendExceptions>() {
        fn declaration<B: BackendClasses + BackendExceptions, T: Declaration<B>>() {}
        fn throw<B: BackendClasses + BackendExceptions>(enter: &Enter<'_, B>) -> Error {
            NetworkError::throw(enter, "offline")
        }

        declaration::<B, ExceptionDeclaration<NetworkError>>();
    }
}
