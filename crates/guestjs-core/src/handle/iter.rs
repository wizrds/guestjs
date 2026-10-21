use std::{
    future::{IntoFuture, poll_fn},
    marker::PhantomData,
    pin::{Pin, pin},
    task::{Context, Poll},
};

use futures::Stream;

use crate::{
    backend::{
        Backend, Val,
        exceptions::BackendExceptions,
        promises::BackendPromises,
        values::{BackendValues, IteratorKind},
    },
    driver::promise::PromiseFuture,
    enter::Enter,
    errors::Error,
    handle::{HasHandle, base::Handle, promise::Awaitable, stream::StreamFuture},
    marshal::{
        FromGuest, ToGuest,
        collections::IterItem,
        describe::{Describe, Expected},
    },
};

pub struct Iter<B: Backend, T> {
    iterator: Handle<B>,
    done: bool,
    target: PhantomData<fn() -> T>,
}

impl<B: BackendValues, T> Iter<B, T> {
    pub fn close(self) -> Result<(), Error> {
        match self.done {
            true => Ok(()),
            false => self
                .iterator
                .with_enter(|enter, iterator| B::close(enter.token(), iterator).map(|_| ())),
        }
    }
}

impl<B, T> Iterator for Iter<B, T>
where
    B: BackendValues,
    T: FromGuest<B>,
{
    type Item = Result<T::Owned, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.done {
            true => None,
            false => {
                let step = self
                    .iterator
                    .with_enter(|enter, iterator| {
                        IterItem::<T>::from_guest(enter, B::next(enter.token(), iterator)?)
                    });

                self.done = !matches!(step, Ok(Some(_)));

                step.transpose()
            }
        }
    }
}

impl<B: Backend, T> HasHandle<B> for Iter<B, T> {
    fn handle(&self) -> &Handle<B> {
        &self.iterator
    }
}

impl<B: Backend, T> Describe for Iter<B, T> {
    fn describe(expected: &mut Expected) {
        expected.push("iterable");
    }
}

impl<B, T> FromGuest<B> for Iter<B, T>
where
    B: BackendValues,
    T: 'static,
{
    type Owned = Self;

    fn from_guest<'js>(enter: &Enter<'js, B>, value: Val<'js, B>) -> Result<Self::Owned, Error> {
        match B::iter(enter.token(), &value, IteratorKind::Sync)? {
            Some(iterator) => Ok(Self {
                iterator: Handle::from_value(enter, iterator),
                done: false,
                target: PhantomData,
            }),
            None => Err(Expected::of::<Self>().mismatch(B::kind(enter.token(), &value))),
        }
    }
}

impl<B: Backend, T> ToGuest<B> for Iter<B, T> {
    fn to_guest<'js>(self, enter: &Enter<'js, B>) -> Result<Val<'js, B>, Error> {
        self.iterator.attach(enter)
    }
}

impl<B: Backend, T> ToGuest<B> for &Iter<B, T> {
    fn to_guest<'js>(self, enter: &Enter<'js, B>) -> Result<Val<'js, B>, Error> {
        self.iterator.attach(enter)
    }
}

pub struct AsyncIter<B: Backend, T: FromGuest<B>> {
    iterator: Handle<B>,
    pending: Option<StreamFuture<Option<T::Owned>>>,
    done: bool,
}

impl<B: Backend, T: FromGuest<B>> Unpin for AsyncIter<B, T> {}

impl<B, T> AsyncIter<B, T>
where
    B: BackendPromises + BackendExceptions,
    T: FromGuest<B> + 'static,
{
    pub async fn next(&mut self) -> Result<Option<T::Owned>, Error> {
        if self.done {
            return Ok(None);
        }

        if self.pending.is_none() {
            match self
                .iterator
                .with_enter(|enter, iterator| {
                    Awaitable::<IterItem<T>>::from_guest(enter, B::next(enter.token(), iterator)?)
                }) {
                Ok(promise) => self.pending = Some(promise.into_future()),
                Err(error) => {
                    self.done = true;

                    return Err(error);
                }
            }
        }

        let step = poll_fn(|context| {
            self.pending
                .as_mut()
                .unwrap()
                .as_mut()
                .poll(context)
        })
        .await;

        self.pending = None;
        self.done = !matches!(&step, Ok(Some(_)));

        step
    }

    pub async fn close(self) -> Result<(), Error> {
        match self.done {
            true => Ok(()),
            false => self
                .iterator
                .with_enter(|enter, iterator| {
                    PromiseFuture::new(enter, B::close(enter.token(), iterator)?)
                })?
                .await
                .map(|_| ()),
        }
    }
}

impl<B, T> Stream for AsyncIter<B, T>
where
    B: BackendPromises + BackendExceptions,
    T: FromGuest<B> + 'static,
{
    type Item = Result<T::Owned, Error>;

    fn poll_next(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        pin!(self.get_mut().next())
            .poll(context)
            .map(Result::transpose)
    }
}

impl<B: Backend, T: FromGuest<B>> HasHandle<B> for AsyncIter<B, T> {
    fn handle(&self) -> &Handle<B> {
        &self.iterator
    }
}

impl<B: Backend, T: FromGuest<B>> Describe for AsyncIter<B, T> {
    fn describe(expected: &mut Expected) {
        expected.push("async iterable");
    }
}

impl<B, T> FromGuest<B> for AsyncIter<B, T>
where
    B: BackendValues,
    T: FromGuest<B> + 'static,
{
    type Owned = Self;

    fn from_guest<'js>(enter: &Enter<'js, B>, value: Val<'js, B>) -> Result<Self::Owned, Error> {
        match B::iter(enter.token(), &value, IteratorKind::Async)? {
            Some(iterator) => Ok(Self {
                iterator: Handle::from_value(enter, iterator),
                pending: None,
                done: false,
            }),
            None => Err(Expected::of::<Self>().mismatch(B::kind(enter.token(), &value))),
        }
    }
}

impl<B: Backend, T: FromGuest<B>> ToGuest<B> for AsyncIter<B, T> {
    fn to_guest<'js>(self, enter: &Enter<'js, B>) -> Result<Val<'js, B>, Error> {
        self.iterator.attach(enter)
    }
}

impl<B: Backend, T: FromGuest<B>> ToGuest<B> for &AsyncIter<B, T> {
    fn to_guest<'js>(self, enter: &Enter<'js, B>) -> Result<Val<'js, B>, Error> {
        self.iterator.attach(enter)
    }
}

#[cfg(test)]
mod tests {
    use futures::Stream;

    use crate::{
        backend::{
            exceptions::BackendExceptions, promises::BackendPromises, tests::Stub,
            values::BackendValues,
        },
        handle::iter::{AsyncIter, Iter},
        marshal::{FromGuest, ToGuest, describe::Expected},
    };

    #[allow(dead_code)]
    fn iter_capabilities<B: BackendValues>() {
        fn iterator<T: Iterator>() {}
        fn from<B: BackendValues, T: FromGuest<B>>() {}
        fn to<B: BackendValues, T: ToGuest<B>>() {}

        iterator::<Iter<B, String>>();
        from::<B, Iter<B, String>>();
        to::<B, Iter<B, String>>();
        to::<B, &Iter<B, String>>();
    }

    #[allow(dead_code)]
    fn async_iter_capabilities<B: BackendPromises + BackendExceptions>() {
        fn stream<T: Stream + Unpin>() {}
        fn from<B: BackendValues, T: FromGuest<B>>() {}
        fn to<B: BackendValues, T: ToGuest<B>>() {}

        stream::<AsyncIter<B, String>>();
        from::<B, AsyncIter<B, String>>();
        to::<B, AsyncIter<B, String>>();
        to::<B, &AsyncIter<B, String>>();
    }

    #[test]
    fn iter_description_names_an_iterable() {
        assert_eq!(Expected::of::<Iter<Stub, String>>().to_string(), "iterable",);
    }

    #[test]
    fn async_iter_description_names_an_async_iterable() {
        assert_eq!(Expected::of::<AsyncIter<Stub, String>>().to_string(), "async iterable",);
    }
}
