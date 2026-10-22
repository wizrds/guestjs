use std::{
    any::type_name,
    cell::{Cell, RefCell, RefMut},
    future::poll_fn,
    marker::PhantomData,
    pin::Pin,
    rc::Rc,
    task::Poll,
};

use futures::{
    FutureExt, Sink, Stream,
    channel::oneshot::{self, Receiver},
};

use crate::{
    backend::{
        Val, callables::BackendCallables, exceptions::BackendExceptions, promises::BackendPromises,
    },
    enter::Enter,
    errors::{BorrowKind, Error},
    handle::{class::Class, object::Object, protocol::ObjectProtocol},
    host::namespace::NamespaceDeclaration,
    marshal::{FromGuest, ToGuest},
};

pub struct SharedStream<S> {
    stream: Rc<RefCell<Option<Pin<Box<S>>>>>,
    turn: Rc<Cell<Option<Receiver<()>>>>,
}

impl<S, T> SharedStream<S>
where
    S: Stream<Item = Result<T, Error>> + 'static,
{
    pub fn new(stream: S) -> Self {
        Self {
            stream: Rc::new(RefCell::new(Some(Box::pin(stream)))),
            turn: Rc::new(Cell::new(None)),
        }
    }

    pub fn next(&self) -> impl Future<Output = Option<Result<T, Error>>> + use<S, T> + 'static {
        let (done, turn) = oneshot::channel::<()>();
        let previous = self.turn.replace(Some(turn));
        let stream = self.stream.clone();

        async move {
            if let Some(previous) = previous {
                let _ = previous.await;
            }

            let step = poll_fn(|context| match stream.borrow_mut().as_mut() {
                Some(stream) => stream.as_mut().poll_next(context),
                None => Poll::Ready(None),
            })
            .await;

            if !matches!(&step, Some(Ok(_))) {
                stream.replace(None);
            }

            drop(done);

            step
        }
    }

    pub fn close(&self) -> Result<(), Error> {
        *self
            .stream
            .try_borrow_mut()
            .map_err(|_| Error::borrow(BorrowKind::Exclusive, type_name::<S>()))? = None;

        Ok(())
    }
}

impl<S> Clone for SharedStream<S> {
    fn clone(&self) -> Self {
        Self {
            stream: self.stream.clone(),
            turn: self.turn.clone(),
        }
    }
}

pub struct SharedSink<S, T> {
    sink: Rc<RefCell<Option<Pin<Box<S>>>>>,
    items: PhantomData<fn(T)>,
}

impl<S, T> SharedSink<S, T>
where
    S: Sink<T, Error = Error> + 'static,
    T: 'static,
{
    pub fn new(sink: S) -> Self {
        Self {
            sink: Rc::new(RefCell::new(Some(Box::pin(sink)))),
            items: PhantomData,
        }
    }

    pub fn send(&self, item: T) -> impl Future<Output = Result<(), Error>> + use<S, T> + 'static {
        let sink = self.clone();

        async move {
            poll_fn(|context| {
                sink.borrow()?
                    .as_mut()
                    .poll_ready(context)
            })
            .await?;

            sink.borrow()?
                .as_mut()
                .start_send(item)?;

            poll_fn(|context| {
                sink.borrow()?
                    .as_mut()
                    .poll_flush(context)
            })
            .await
        }
    }

    pub fn close(&self) -> impl Future<Output = Result<(), Error>> + use<S, T> + 'static {
        let sink = self.clone();

        async move {
            poll_fn(|context| {
                sink.borrow()?
                    .as_mut()
                    .poll_close(context)
            })
            .await
        }
    }

    pub fn abort(&self) -> Result<(), Error> {
        *self
            .sink
            .try_borrow_mut()
            .map_err(|_| Error::borrow(BorrowKind::Exclusive, type_name::<S>()))? = None;

        Ok(())
    }

    fn borrow(&self) -> Result<RefMut<'_, Pin<Box<S>>>, Error> {
        RefMut::filter_map(
            self.sink
                .try_borrow_mut()
                .map_err(|_| Error::borrow(BorrowKind::Exclusive, type_name::<S>()))?,
            Option::as_mut,
        )
        .map_err(|_| Error::unexpected("sink was aborted"))
    }
}

impl<S, T> Clone for SharedSink<S, T> {
    fn clone(&self) -> Self {
        Self {
            sink: self.sink.clone(),
            items: PhantomData,
        }
    }
}

pub struct StreamSource<S> {
    stream: S,
}

impl<S> StreamSource<S> {
    pub fn new(stream: S) -> Self {
        Self { stream }
    }
}

impl<B, S, T> ToGuest<B> for StreamSource<S>
where
    B: BackendCallables + BackendPromises + BackendExceptions,
    S: Stream<Item = Result<T, Error>> + 'static,
    T: ToGuest<B> + 'static,
{
    fn to_guest<'js>(self, enter: &Enter<'js, B>) -> Result<Val<'js, B>, Error> {
        let stream = SharedStream::new(self.stream);
        let cancelling = stream.clone();
        let mut underlying = NamespaceDeclaration::new();

        underlying
            .function_async("pull", move |context, args| {
                let controller = args.next::<Object<B>>(context.enter())?;

                Ok(stream
                    .next()
                    .map(move |step| match step {
                        Some(Ok(chunk)) => controller.call_method::<_, _, ()>("enqueue", (chunk,)),
                        Some(Err(error)) => Err(error),
                        None => controller.call_method::<_, _, ()>("close", ()),
                    }))
            })
            .function("cancel", move |_, _| cancelling.close());

        Object::<B>::from_guest(enter, B::globals(enter.token()))?
            .get::<Class<B, Object<B>>, _>("ReadableStream")?
            .construct((underlying,))?
            .to_guest(enter)
    }
}

pub struct StreamSink<S, T> {
    sink: S,
    chunks: PhantomData<fn(T)>,
}

impl<S, T> StreamSink<S, T> {
    pub fn new(sink: S) -> Self {
        Self { sink, chunks: PhantomData }
    }
}

impl<B, S, T> ToGuest<B> for StreamSink<S, T>
where
    B: BackendCallables + BackendPromises + BackendExceptions,
    S: Sink<T::Owned, Error = Error> + 'static,
    T: FromGuest<B> + 'static,
    T::Owned: 'static,
{
    fn to_guest<'js>(self, enter: &Enter<'js, B>) -> Result<Val<'js, B>, Error> {
        let sink = SharedSink::new(self.sink);
        let closing = sink.clone();
        let aborting = sink.clone();
        let mut underlying = NamespaceDeclaration::new();

        underlying
            .function_async("write", move |context, args| {
                Ok(sink.send(args.next::<T>(context.enter())?))
            })
            .function_async("close", move |_, _| Ok(closing.close()))
            .function("abort", move |_, _| aborting.abort());

        Object::<B>::from_guest(enter, B::globals(enter.token()))?
            .get::<Class<B, Object<B>>, _>("WritableStream")?
            .construct((underlying,))?
            .to_guest(enter)
    }
}

#[cfg(test)]
mod tests {
    use std::vec::IntoIter;

    use futures::{
        SinkExt, StreamExt,
        channel::mpsc::{self, SendError, UnboundedSender},
        sink::SinkMapErr,
        stream::{self, Iter},
    };

    use crate::{
        backend::{
            Backend, callables::BackendCallables, exceptions::BackendExceptions,
            promises::BackendPromises,
        },
        errors::Error,
        host::stream::{SharedSink, SharedStream, StreamSink, StreamSource},
        marshal::ToGuest,
    };

    #[tokio::test]
    async fn shared_stream_keeps_call_order() {
        let shared = SharedStream::new(stream::iter([1, 2, 3]).map(Ok::<_, Error>));
        let first = shared.next();
        let second = shared.next();
        let third = shared.next();

        let (third, second, first) = tokio::join!(third, second, first);

        assert!(matches!(first, Some(Ok(1))));
        assert!(matches!(second, Some(Ok(2))));
        assert!(matches!(third, Some(Ok(3))));
    }

    #[tokio::test]
    async fn shared_stream_ends_after_error() {
        let shared =
            SharedStream::new(stream::iter([Ok(1), Err(Error::unexpected("failed")), Ok(2)]));

        assert!(matches!(shared.next().await, Some(Ok(1))));
        assert!(matches!(shared.next().await, Some(Err(Error::Unexpected { .. }))));
        assert!(shared.next().await.is_none());
    }

    #[tokio::test]
    async fn shared_stream_close_ends_stream() {
        let shared = SharedStream::new(stream::iter([1]).map(Ok::<_, Error>));

        shared.close().unwrap();

        assert!(shared.next().await.is_none());
    }

    #[tokio::test]
    async fn shared_sink_sends_and_closes() {
        let (sender, receiver) = mpsc::unbounded::<i64>();
        let shared =
            SharedSink::new(sender.sink_map_err(|error| Error::unexpected(error.to_string())));

        shared.send(1).await.unwrap();
        shared.send(2).await.unwrap();
        shared.close().await.unwrap();

        assert_eq!(receiver.collect::<Vec<_>>().await, vec![1, 2]);
    }

    #[tokio::test]
    async fn shared_sink_rejects_after_abort() {
        let (sender, _receiver) = mpsc::unbounded::<i64>();
        let shared =
            SharedSink::new(sender.sink_map_err(|error| Error::unexpected(error.to_string())));

        shared.abort().unwrap();

        assert!(matches!(shared.send(1).await, Err(Error::Unexpected { .. })));
    }

    #[allow(dead_code)]
    fn capabilities<B>()
    where
        B: BackendCallables + BackendPromises + BackendExceptions,
    {
        fn to<B: Backend, T: ToGuest<B>>() {}

        to::<B, StreamSource<Iter<IntoIter<Result<String, Error>>>>>();
        to::<B, StreamSink<SinkMapErr<UnboundedSender<String>, fn(SendError) -> Error>, String>>();
    }
}
