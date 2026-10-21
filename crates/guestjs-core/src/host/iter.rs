use std::{any::type_name, cell::RefCell, future::ready, rc::Rc};

use futures::{FutureExt, Stream};

use crate::{
    backend::{
        Val,
        callables::BackendCallables,
        exceptions::BackendExceptions,
        promises::BackendPromises,
        values::WellKnown,
    },
    enter::Enter,
    errors::{BorrowKind, Error},
    handle::object::Object,
    host::{namespace::NamespaceDeclaration, stream::SharedStream},
    marshal::{ToGuest, collections::IterItem},
};

pub struct Iterable<I> {
    iterator: I,
}

impl<I> Iterable<I> {
    pub fn new(iterator: I) -> Self {
        Self { iterator }
    }
}

impl<B, I, T> ToGuest<B> for Iterable<I>
where
    B: BackendCallables,
    I: Iterator<Item = T> + 'static,
    T: ToGuest<B> + 'static,
{
    fn to_guest<'js>(self, enter: &Enter<'js, B>) -> Result<Val<'js, B>, Error> {
        let iterator = Rc::new(RefCell::new(Some(self.iterator)));
        let closing = iterator.clone();
        let mut namespace = NamespaceDeclaration::new();

        namespace
            .function("next", move |_, _| {
                let mut iterator = iterator
                    .try_borrow_mut()
                    .map_err(|_| Error::borrow(BorrowKind::Exclusive, type_name::<I>()))?;
                let item = iterator
                    .as_mut()
                    .and_then(Iterator::next);

                if item.is_none() {
                    *iterator = None;
                }

                Ok(IterItem::new(item))
            })
            .function("return", move |_, _| {
                *closing
                    .try_borrow_mut()
                    .map_err(|_| Error::borrow(BorrowKind::Exclusive, type_name::<I>()))? = None;

                Ok(IterItem::<T>::new(None))
            })
            .function(WellKnown::Iterator, |context, args| {
                args.this::<Object<B>>(context.enter())
            });

        namespace.to_guest(enter)
    }
}

pub struct AsyncIterable<S> {
    stream: S,
}

impl<S> AsyncIterable<S> {
    pub fn new(stream: S) -> Self {
        Self { stream }
    }
}

impl<B, S, T> ToGuest<B> for AsyncIterable<S>
where
    B: BackendCallables + BackendPromises + BackendExceptions,
    S: Stream<Item = Result<T, Error>> + 'static,
    T: ToGuest<B> + 'static,
{
    fn to_guest<'js>(self, enter: &Enter<'js, B>) -> Result<Val<'js, B>, Error> {
        let stream = SharedStream::new(self.stream);
        let closing = stream.clone();
        let mut namespace = NamespaceDeclaration::new();

        namespace
            .function_async("next", move |_, _| {
                Ok(stream
                    .next()
                    .map(|step| step.transpose().map(IterItem::new)))
            })
            .function_async("return", move |_, _| {
                closing.close()?;

                Ok(ready(Ok(IterItem::<T>::new(None))))
            })
            .function(WellKnown::AsyncIterator, |context, args| {
                args.this::<Object<B>>(context.enter())
            });

        namespace.to_guest(enter)
    }
}

#[cfg(test)]
mod tests {
    use std::{ops::Range, vec::IntoIter};

    use futures::stream::Iter;

    use crate::{
        backend::{
            Backend,
            callables::BackendCallables,
            exceptions::BackendExceptions,
            promises::BackendPromises,
        },
        errors::Error,
        host::iter::{AsyncIterable, Iterable},
        marshal::ToGuest,
    };

    #[allow(dead_code)]
    fn capabilities<B>()
    where
        B: BackendCallables + BackendPromises + BackendExceptions,
    {
        fn to<B: Backend, T: ToGuest<B>>() {}

        to::<B, Iterable<Range<i64>>>();
        to::<B, Iterable<IntoIter<String>>>();
        to::<B, AsyncIterable<Iter<IntoIter<Result<String, Error>>>>>();
    }
}
