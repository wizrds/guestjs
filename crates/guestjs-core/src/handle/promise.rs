use std::{
    future::{Future, IntoFuture},
    marker::PhantomData,
    pin::Pin,
};

use crate::{
    backend::{
        Backend, Val,
        exceptions::BackendExceptions,
        promises::BackendPromises,
        values::{BackendValues, Kind},
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

pub struct Promise<B: Backend, T> {
    handle: Handle<B>,
    target: PhantomData<fn() -> T>,
}

impl<B: BackendPromises, T: 'static> Promise<B, T> {
    pub fn resolve_value<'js>(enter: &Enter<'js, B>, value: Val<'js, B>) -> Result<Self, Error> {
        Self::from_guest(enter, B::resolve(enter.token(), value)?)
    }

    pub fn resolve<V>(enter: &Enter<'_, B>, value: V) -> Result<Self, Error>
    where
        V: ToGuest<B>,
    {
        Self::resolve_value(enter, value.to_guest(enter)?)
    }
}

impl<B: Backend, T> Clone for Promise<B, T> {
    fn clone(&self) -> Self {
        Self {
            handle: self.handle.clone(),
            target: PhantomData,
        }
    }
}

impl<B: Backend, T> HasHandle<B> for Promise<B, T> {
    fn handle(&self) -> &Handle<B> {
        &self.handle
    }
}

impl<B: BackendValues, T> ObjectProtocol<B> for Promise<B, T> {}

impl<B, T> IntoFuture for Promise<B, T>
where
    B: BackendPromises + BackendExceptions,
    T: FromGuest<B> + 'static,
{
    type Output = Result<T::Owned, Error>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output>>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            self.handle
                .with_enter(|enter, promise| PromiseFuture::new(enter, promise.clone()))?
                .await
                .and_then(|owned| {
                    self.handle
                        .guest()
                        .enter(|enter| T::from_guest(enter, B::attach(enter.token(), &owned)?))
                })
        })
    }
}

impl<B: Backend, T> Describe for Promise<B, T> {
    fn describe(expected: &mut Expected) {
        expected.push("promise");
    }
}

impl<B, T> FromGuest<B> for Promise<B, T>
where
    B: BackendValues,
    T: 'static,
{
    type Owned = Self;

    fn from_guest<'js>(enter: &Enter<'js, B>, value: Val<'js, B>) -> Result<Self::Owned, Error> {
        match B::kind(enter.token(), &value) {
            Kind::Promise => Ok(Self {
                handle: Handle::from_value(enter, value),
                target: PhantomData,
            }),
            kind => Err(Expected::of::<Self>().mismatch(kind)),
        }
    }
}

impl<B: Backend, T> ToGuest<B> for Promise<B, T> {
    fn to_guest<'js>(self, enter: &Enter<'js, B>) -> Result<Val<'js, B>, Error> {
        self.handle.attach(enter)
    }
}

impl<B: Backend, T> ToGuest<B> for &Promise<B, T> {
    fn to_guest<'js>(self, enter: &Enter<'js, B>) -> Result<Val<'js, B>, Error> {
        self.handle.attach(enter)
    }
}

pub struct Awaitable<T>(PhantomData<fn() -> T>);

impl<T> Describe for Awaitable<T> {
    fn describe(expected: &mut Expected) {
        expected.push("awaitable");
    }
}

impl<B: BackendPromises, T: 'static> FromGuest<B> for Awaitable<T> {
    type Owned = Promise<B, T>;

    fn from_guest<'js>(enter: &Enter<'js, B>, value: Val<'js, B>) -> Result<Self::Owned, Error> {
        Promise::resolve_value(enter, value)
    }
}

#[cfg(test)]
mod tests {
    use std::future::IntoFuture;

    use crate::{
        backend::{exceptions::BackendExceptions, promises::BackendPromises, tests::Stub},
        handle::{promise::Promise, protocol::ObjectProtocol},
        marshal::{FromGuest, ToGuest, describe::Expected},
    };

    #[allow(dead_code)]
    fn capabilities<B: BackendPromises + BackendExceptions>() {
        fn object<B: BackendPromises, T: ObjectProtocol<B>>() {}
        fn from<B: BackendPromises, T: FromGuest<B>>() {}
        fn to<B: BackendPromises, T: ToGuest<B>>() {}
        fn awaitable<T: IntoFuture>() {}

        object::<B, Promise<B, String>>();
        from::<B, Promise<B, String>>();
        to::<B, Promise<B, String>>();
        to::<B, &Promise<B, String>>();
        awaitable::<Promise<B, String>>();
    }

    #[test]
    fn description_names_a_promise() {
        assert_eq!(Expected::of::<Promise<Stub, String>>().to_string(), "promise",);
    }
}
