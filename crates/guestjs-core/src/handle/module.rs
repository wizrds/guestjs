use std::{
    future::{Future, IntoFuture},
    pin::Pin,
};

use crate::{
    backend::{
        Backend, Val,
        exceptions::BackendExceptions,
        promises::BackendPromises,
        values::{BackendValues, Kind, WellKnown},
    },
    driver::promise::PromiseFuture,
    enter::Enter,
    errors::Error,
    handle::{HasHandle, base::Handle, protocol::ObjectProtocol},
    marshal::{
        FromGuest, ToGuest,
        describe::{Describe, Expected},
    },
};

pub struct Module<B: Backend> {
    handle: Handle<B>,
    evaluation: Option<Handle<B>>,
}

impl<B: BackendValues> Module<B> {
    pub fn evaluating<'js>(
        enter: &Enter<'js, B>,
        namespace: Val<'js, B>,
        evaluation: Val<'js, B>,
    ) -> Result<Self, Error> {
        Ok(Self {
            evaluation: Some(Handle::from_value(enter, evaluation)),
            ..Self::from_guest(enter, namespace)?
        })
    }
}

impl<B: Backend> Clone for Module<B> {
    fn clone(&self) -> Self {
        Self {
            handle: self.handle.clone(),
            evaluation: self.evaluation.clone(),
        }
    }
}

impl<B: Backend> HasHandle<B> for Module<B> {
    fn handle(&self) -> &Handle<B> {
        &self.handle
    }
}

impl<B: BackendValues> ObjectProtocol<B> for Module<B> {}

impl<B> IntoFuture for Module<B>
where
    B: BackendPromises + BackendExceptions,
{
    type Output = Result<Self, Error>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output>>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            if let Some(evaluation) = self.evaluation {
                evaluation
                    .with_enter(|enter, promise| PromiseFuture::new(enter, promise.clone()))?
                    .await?;
            }

            Ok(Self { handle: self.handle, evaluation: None })
        })
    }
}

impl<B: Backend> Describe for Module<B> {
    fn describe(expected: &mut Expected) {
        expected.push("module namespace");
    }
}

impl<B: BackendValues> FromGuest<B> for Module<B> {
    type Owned = Self;

    fn from_guest<'js>(enter: &Enter<'js, B>, value: Val<'js, B>) -> Result<Self::Owned, Error> {
        match B::kind(enter.token(), &value) {
            Kind::Object => match Option::<String>::from_guest(
                enter,
                B::get(
                    enter.token(),
                    &value,
                    &B::well_known(enter.token(), WellKnown::ToStringTag),
                )?,
            )?
            .as_deref()
            {
                Some("Module") => Ok(Self {
                    handle: Handle::from_value(enter, value),
                    evaluation: None,
                }),
                _ => Err(Expected::of::<Self>().mismatch(Kind::Object)),
            },
            kind => Err(Expected::of::<Self>().mismatch(kind)),
        }
    }
}

impl<B: Backend> ToGuest<B> for Module<B> {
    fn to_guest<'js>(self, enter: &Enter<'js, B>) -> Result<Val<'js, B>, Error> {
        self.handle.attach(enter)
    }
}

impl<B: Backend> ToGuest<B> for &Module<B> {
    fn to_guest<'js>(self, enter: &Enter<'js, B>) -> Result<Val<'js, B>, Error> {
        self.handle.attach(enter)
    }
}

#[cfg(test)]
mod tests {
    use std::future::IntoFuture;

    use crate::{
        backend::{exceptions::BackendExceptions, promises::BackendPromises, tests::Stub},
        handle::{module::Module, protocol::ObjectProtocol},
        marshal::{FromGuest, ToGuest, describe::Expected},
    };

    #[allow(dead_code)]
    fn capabilities<B: BackendPromises + BackendExceptions>() {
        fn object<B: BackendPromises, T: ObjectProtocol<B>>() {}
        fn from<B: BackendPromises, T: FromGuest<B>>() {}
        fn to<B: BackendPromises, T: ToGuest<B>>() {}
        fn awaitable<T: IntoFuture>() {}

        object::<B, Module<B>>();
        from::<B, Module<B>>();
        to::<B, Module<B>>();
        to::<B, &Module<B>>();
        awaitable::<Module<B>>();
    }

    #[test]
    fn description_names_a_module_namespace() {
        assert_eq!(Expected::of::<Module<Stub>>().to_string(), "module namespace",);
    }
}
