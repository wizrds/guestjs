use std::marker::PhantomData;

use crate::{
    backend::{
        exceptions::BackendExceptions,
        promises::BackendPromises,
        values::{
            BackendValues,
            Kind,
            WellKnown,
        },
        Backend,
        Val,
    },
    driver::promise::PromiseFuture,
    enter::Enter,
    errors::Error,
    handle::{
        base::Handle,
        HasHandle,
    },
    marshal::{
        describe::{
            Describe,
            Expected,
        },
        FromGuest,
        ToGuest,
    },
};

pub struct IterItem<T> {
    target: PhantomData<fn() -> T>,
}

impl<T> Describe for IterItem<T> {
    fn describe(expected: &mut Expected) {
        expected.push("iter item");
    }
}

impl<B, T> FromGuest<B> for IterItem<T>
where
    B: BackendValues,
    T: FromGuest<B>,
{
    type Owned = Option<T::Owned>;

    fn from_guest<'js>(
        enter: &Enter<'js, B>,
        value: Val<'js, B>,
    ) -> Result<Self::Owned, Error> {
        match B::kind(enter.token(), &value) {
            kind if kind.is_object() => match Option::<bool>::from_guest(
                enter,
                B::get(enter.token(), &value, &"done".to_guest(enter)?)?,
            )? {
                Some(true) => Ok(None),
                _ => T::from_guest(
                    enter,
                    B::get(enter.token(), &value, &"value".to_guest(enter)?)?,
                )
                .map(Some),
            },
            kind => Err(Expected::of::<Self>().mismatch(kind)),
        }
    }
}

pub struct Iter<B: Backend, T> {
    iterator: Handle<B>,
    next: Handle<B>,
    done: bool,
    target: PhantomData<fn() -> T>,
}

impl<B: BackendValues, T> Iter<B, T> {
    pub fn close(self) -> Result<(), Error> {
        match self.done {
            true => Ok(()),
            false => self.iterator.with_enter(|enter, iterator| {
                let method = B::get(
                    enter.token(),
                    iterator,
                    &"return".to_guest(enter)?,
                )?;

                match B::kind(enter.token(), &method) {
                    Kind::Function => {
                        B::call(enter.token(), &method, iterator, &[])
                            .map(|_| ())
                    }
                    _ => Ok(()),
                }
            }),
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
                let step = self.iterator.with_enter(|enter, iterator| {
                    IterItem::<T>::from_guest(
                        enter,
                        B::call(
                            enter.token(),
                            &self.next.attach(enter)?,
                            iterator,
                            &[],
                        )?,
                    )
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

    fn from_guest<'js>(
        enter: &Enter<'js, B>,
        value: Val<'js, B>,
    ) -> Result<Self::Owned, Error> {
        let token = enter.token();

        match B::kind(token, &value) {
            kind if kind.is_object() => {
                let method = B::get(
                    token,
                    &value,
                    &B::well_known(token, WellKnown::Iterator),
                )?;

                match B::kind(token, &method) {
                    Kind::Function => {
                        let iterator = B::call(token, &method, &value, &[])?;

                        Ok(Self {
                            next: Handle::from_value(
                                enter,
                                B::get(
                                    token,
                                    &iterator,
                                    &"next".to_guest(enter)?,
                                )?,
                            ),
                            iterator: Handle::from_value(enter, iterator),
                            done: false,
                            target: PhantomData,
                        })
                    }
                    _ => Err(Expected::of::<Self>().mismatch(kind)),
                }
            }
            kind => Err(Expected::of::<Self>().mismatch(kind)),
        }
    }
}

impl<B: Backend, T> ToGuest<B> for Iter<B, T> {
    fn to_guest<'js>(
        self,
        enter: &Enter<'js, B>,
    ) -> Result<Val<'js, B>, Error> {
        self.iterator.attach(enter)
    }
}

impl<B: Backend, T> ToGuest<B> for &Iter<B, T> {
    fn to_guest<'js>(
        self,
        enter: &Enter<'js, B>,
    ) -> Result<Val<'js, B>, Error> {
        self.iterator.attach(enter)
    }
}

pub struct AsyncIter<B: Backend, T> {
    iterator: Handle<B>,
    next: Handle<B>,
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
                let step = match self.iterator.with_enter(|enter, iterator| {
                    PromiseFuture::new(
                        enter,
                        B::call(
                            enter.token(),
                            &self.next.attach(enter)?,
                            iterator,
                            &[],
                        )?,
                    )
                }) {
                    Ok(future) => future.await.and_then(|owned| {
                        self.iterator.guest().enter(|enter| {
                            IterItem::<T>::from_guest(
                                enter,
                                B::attach(enter.token(), &owned)?,
                            )
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
            false => match self.iterator.with_enter(|enter, iterator| {
                let method = B::get(
                    enter.token(),
                    iterator,
                    &"return".to_guest(enter)?,
                )?;

                match B::kind(enter.token(), &method) {
                    Kind::Function => PromiseFuture::new(
                        enter,
                        B::call(enter.token(), &method, iterator, &[])?,
                    )
                    .map(Some),
                    _ => Ok(None),
                }
            })? {
                Some(future) => future.await.map(|_| ()),
                None => Ok(()),
            },
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

    fn from_guest<'js>(
        enter: &Enter<'js, B>,
        value: Val<'js, B>,
    ) -> Result<Self::Owned, Error> {
        let token = enter.token();

        match B::kind(token, &value) {
            kind if kind.is_object() => {
                let method = B::get(
                    token,
                    &value,
                    &B::well_known(token, WellKnown::AsyncIterator),
                )?;

                match B::kind(token, &method) {
                    Kind::Function => {
                        let iterator = B::call(token, &method, &value, &[])?;

                        Ok(Self {
                            next: Handle::from_value(
                                enter,
                                B::get(
                                    token,
                                    &iterator,
                                    &"next".to_guest(enter)?,
                                )?,
                            ),
                            iterator: Handle::from_value(enter, iterator),
                            done: false,
                            target: PhantomData,
                        })
                    }
                    _ => Err(Expected::of::<Self>().mismatch(kind)),
                }
            }
            kind => Err(Expected::of::<Self>().mismatch(kind)),
        }
    }
}

impl<B: Backend, T> ToGuest<B> for AsyncIter<B, T> {
    fn to_guest<'js>(
        self,
        enter: &Enter<'js, B>,
    ) -> Result<Val<'js, B>, Error> {
        self.iterator.attach(enter)
    }
}

impl<B: Backend, T> ToGuest<B> for &AsyncIter<B, T> {
    fn to_guest<'js>(
        self,
        enter: &Enter<'js, B>,
    ) -> Result<Val<'js, B>, Error> {
        self.iterator.attach(enter)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        backend::{
            tests::Stub,
            values::BackendValues,
        },
        handle::iter::{
            AsyncIter,
            Iter,
            IterItem,
        },
        marshal::{
            describe::Expected,
            FromGuest,
            ToGuest,
        },
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
    fn item_description_names_an_iter_item() {
        assert_eq!(
            Expected::of::<IterItem<String>>().to_string(),
            "iter item",
        );
    }

    #[test]
    fn iter_description_names_an_iterable() {
        assert_eq!(
            Expected::of::<Iter<Stub, String>>().to_string(),
            "iterable",
        );
    }

    #[test]
    fn async_iter_description_names_an_async_iterable() {
        assert_eq!(
            Expected::of::<AsyncIter<Stub, String>>().to_string(),
            "async iterable",
        );
    }
}
