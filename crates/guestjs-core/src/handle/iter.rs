use std::marker::PhantomData;

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
    handle::{HasHandle, base::Handle},
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

pub struct AsyncIter<B: Backend, T> {
    iterator: Handle<B>,
    done: bool,
    target: PhantomData<fn() -> T>,
}

impl<B, T> AsyncIter<B, T>
where
    B: BackendPromises + BackendExceptions,
    T: FromGuest<B>,
{
    pub async fn next(&mut self) -> Result<Option<T::Owned>, Error> {
        match self.done {
            true => Ok(None),
            false => {
                let step = match self
                    .iterator
                    .with_enter(|enter, iterator| {
                        PromiseFuture::new(enter, B::next(enter.token(), iterator)?)
                    }) {
                    Ok(future) => future.await.and_then(|owned| {
                        self.iterator.guest().enter(|enter| {
                            IterItem::<T>::from_guest(enter, B::attach(enter.token(), &owned)?)
                        })
                    }),
                    Err(error) => Err(error),
                };

                self.done = !matches!(step, Ok(Some(_)));

                step
            }
        }
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

impl<B: Backend, T> HasHandle<B> for AsyncIter<B, T> {
    fn handle(&self) -> &Handle<B> {
        &self.iterator
    }
}

impl<B: Backend, T> Describe for AsyncIter<B, T> {
    fn describe(expected: &mut Expected) {
        expected.push("async iterable");
    }
}

impl<B, T> FromGuest<B> for AsyncIter<B, T>
where
    B: BackendValues,
    T: 'static,
{
    type Owned = Self;

    fn from_guest<'js>(enter: &Enter<'js, B>, value: Val<'js, B>) -> Result<Self::Owned, Error> {
        match B::iter(enter.token(), &value, IteratorKind::Async)? {
            Some(iterator) => Ok(Self {
                iterator: Handle::from_value(enter, iterator),
                done: false,
                target: PhantomData,
            }),
            None => Err(Expected::of::<Self>().mismatch(B::kind(enter.token(), &value))),
        }
    }
}

impl<B: Backend, T> ToGuest<B> for AsyncIter<B, T> {
    fn to_guest<'js>(self, enter: &Enter<'js, B>) -> Result<Val<'js, B>, Error> {
        self.iterator.attach(enter)
    }
}

impl<B: Backend, T> ToGuest<B> for &AsyncIter<B, T> {
    fn to_guest<'js>(self, enter: &Enter<'js, B>) -> Result<Val<'js, B>, Error> {
        self.iterator.attach(enter)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        backend::{tests::Stub, values::BackendValues},
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
    fn async_iter_capabilities<B: BackendValues>() {
        fn from<B: BackendValues, T: FromGuest<B>>() {}
        fn to<B: BackendValues, T: ToGuest<B>>() {}

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
