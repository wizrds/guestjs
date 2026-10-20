use std::{
    future::{IntoFuture, poll_fn},
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};

use futures::{Sink, SinkExt, Stream, StreamExt};

use crate::{
    backend::{
        Backend, Val, exceptions::BackendExceptions, promises::BackendPromises,
        values::BackendValues,
    },
    enter::Enter,
    errors::Error,
    handle::{HasHandle, base::Handle, object::Object, promise::Promise, protocol::ObjectProtocol},
    marshal::{
        FromGuest, ToGuest,
        collections::IterItem,
        describe::{Describe, Expected},
    },
};

type StreamFuture<T> = Pin<Box<dyn Future<Output = Result<T, Error>>>>;

struct StreamPromise<T>(PhantomData<fn() -> T>);

impl<B: BackendPromises, T: 'static> FromGuest<B> for StreamPromise<T> {
    type Owned = Promise<B, T>;

    fn from_guest<'js>(enter: &Enter<'js, B>, value: Val<'js, B>) -> Result<Self::Owned, Error> {
        Promise::resolve_value(enter, value)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ReaderPhase {
    Open,
    Done,
    Released,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum WriterPhase {
    Open,
    Writing,
    Closing,
    Closed,
    Failed,
    Released,
}

struct WriterState {
    phase: WriterPhase,
    pending: Option<StreamFuture<()>>,
}

impl WriterState {
    fn new() -> Self {
        Self { phase: WriterPhase::Open, pending: None }
    }

    fn phase(&self) -> WriterPhase {
        self.phase
    }

    fn begin(&mut self, phase: WriterPhase, future: StreamFuture<()>) {
        self.phase = phase;
        self.pending = Some(future);
    }

    fn reset(&mut self, phase: WriterPhase) {
        self.phase = phase;
        self.pending = None;
    }

    fn poll(&mut self, context: &mut Context<'_>) -> Poll<Result<(), Error>> {
        match self
            .pending
            .as_mut()
            .map(|future| future.as_mut().poll(context))
        {
            None => Poll::Ready(Ok(())),
            Some(Poll::Pending) => Poll::Pending,
            Some(Poll::Ready(result)) => {
                self.reset(match (&result, self.phase) {
                    (Err(_), _) => WriterPhase::Failed,
                    (Ok(()), WriterPhase::Writing) => WriterPhase::Open,
                    (Ok(()), WriterPhase::Closing) => WriterPhase::Closed,
                    _ => self.phase,
                });

                Poll::Ready(result)
            }
        }
    }
}

macro_rules! stream_handle {
    (
        $name:ident<$backend:ident, $($chunk:ident),+>, $description:literal,
        methods [$($method:literal),*],
        properties [$($property:literal => $descriptor:ty),*]
    ) => {
        pub struct $name<$backend: Backend, $($chunk),+> {
            object: Object<$backend>,
            chunks: PhantomData<fn() -> ($($chunk,)+)>,
        }

        impl<$backend: Backend, $($chunk),+> Clone
            for $name<$backend, $($chunk),+>
        {
            fn clone(&self) -> Self {
                Self {
                    object: self.object.clone(),
                    chunks: PhantomData,
                }
            }
        }

        impl<$backend: Backend, $($chunk),+> HasHandle<$backend>
            for $name<$backend, $($chunk),+>
        {
            fn handle(&self) -> &Handle<$backend> {
                self.object.handle()
            }
        }

        impl<$backend: BackendValues, $($chunk),+> ObjectProtocol<$backend>
            for $name<$backend, $($chunk),+>
        {}

        impl<$backend: Backend, $($chunk),+> Describe
            for $name<$backend, $($chunk),+>
        {
            fn describe(expected: &mut Expected) {
                expected.push($description);
            }
        }

        impl<$backend: BackendValues, $($chunk: 'static),+> FromGuest<$backend>
            for $name<$backend, $($chunk),+>
        {
            type Owned = Self;

            fn from_guest<'js>(
                enter: &Enter<'js, $backend>,
                value: Val<'js, $backend>,
            ) -> Result<Self::Owned, Error> {
                let object = Object::from_guest(enter, value)?;

                $(object.function($method)?;)*
                $(object.get::<$descriptor, _>($property)?;)*

                Ok(Self {
                    object,
                    chunks: PhantomData,
                })
            }
        }

        impl<$backend: Backend, $($chunk),+> ToGuest<$backend>
            for $name<$backend, $($chunk),+>
        {
            fn to_guest<'js>(
                self,
                enter: &Enter<'js, $backend>,
            ) -> Result<Val<'js, $backend>, Error> {
                self.object.to_guest(enter)
            }
        }

        impl<$backend: Backend, $($chunk),+> ToGuest<$backend>
            for &$name<$backend, $($chunk),+>
        {
            fn to_guest<'js>(
                self,
                enter: &Enter<'js, $backend>,
            ) -> Result<Val<'js, $backend>, Error> {
                (&self.object).to_guest(enter)
            }
        }
    };
}

stream_handle!(
    ReadableStream<B, T>, "ReadableStream",
    methods ["getReader", "cancel", "tee", "pipeTo", "pipeThrough"],
    properties []
);

impl<B: BackendValues, T: 'static> ReadableStream<B, T> {
    pub fn locked(&self) -> Result<bool, Error> {
        self.object.get::<bool, _>("locked")
    }

    pub fn get_reader(&self) -> Result<Reader<B, T>, Error>
    where
        T: FromGuest<B>,
    {
        self.object
            .call_method::<_, _, Reader<B, T>>("getReader", ())
    }

    pub fn tee(&self) -> Result<(Self, Self), Error> {
        self.object
            .call_method::<_, _, (Self, Self)>("tee", ())
    }

    pub fn pipe_through<I, O, P>(
        &self,
        transform: &TransformStream<B, I, O>,
        options: P,
    ) -> Result<ReadableStream<B, O>, Error>
    where
        I: 'static,
        O: 'static,
        P: ToGuest<B>,
    {
        self.object
            .call_method::<_, _, ReadableStream<B, O>>("pipeThrough", (transform, options))
    }
}

impl<B: BackendPromises, T: 'static> ReadableStream<B, T> {
    pub fn cancel<R>(&self, reason: R) -> Result<Promise<B, ()>, Error>
    where
        R: ToGuest<B>,
    {
        self.object
            .call_method::<_, _, StreamPromise<()>>("cancel", (reason,))
    }

    pub fn pipe_to<I, P>(
        &self,
        destination: &WritableStream<B, I>,
        options: P,
    ) -> Result<Promise<B, ()>, Error>
    where
        I: 'static,
        P: ToGuest<B>,
    {
        self.object
            .call_method::<_, _, StreamPromise<()>>("pipeTo", (destination, options))
    }
}

pub struct Reader<B: Backend, T: FromGuest<B>> {
    object: Object<B>,
    pending: Option<StreamFuture<Option<T::Owned>>>,
    phase: ReaderPhase,
}

impl<B: Backend, T: FromGuest<B>> Unpin for Reader<B, T> {}

impl<B: BackendValues, T: FromGuest<B>> Reader<B, T> {
    pub fn release_lock(&mut self) -> Result<(), Error> {
        self.object
            .call_method::<_, _, ()>("releaseLock", ())?;

        self.phase = ReaderPhase::Released;
        self.pending = None;

        Ok(())
    }
}

impl<B: BackendPromises, T: FromGuest<B>> Reader<B, T> {
    pub fn cancel<R>(&self, reason: R) -> Result<Promise<B, ()>, Error>
    where
        R: ToGuest<B>,
    {
        self.object
            .call_method::<_, _, StreamPromise<()>>("cancel", (reason,))
    }

    pub fn closed(&self) -> Result<Promise<B, ()>, Error> {
        self.object
            .get::<StreamPromise<()>, _>("closed")
    }
}

impl<B, T> Reader<B, T>
where
    B: BackendPromises + BackendExceptions,
    T: FromGuest<B> + 'static,
{
    pub async fn read(&mut self) -> Result<Option<T::Owned>, Error> {
        if self.phase == ReaderPhase::Released {
            return Err(Error::unexpected("reader lock was released"));
        }

        self.next().await.transpose()
    }
}

impl<B, T> Stream for Reader<B, T>
where
    B: BackendPromises + BackendExceptions,
    T: FromGuest<B> + 'static,
{
    type Item = Result<T::Owned, Error>;

    fn poll_next(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();

        if this.phase != ReaderPhase::Open {
            return Poll::Ready(None);
        }

        if this.pending.is_none() {
            match this
                .object
                .call_method::<_, _, StreamPromise<IterItem<T>>>("read", ())
            {
                Ok(promise) => this.pending = Some(promise.into_future()),
                Err(error) => {
                    this.phase = ReaderPhase::Done;

                    return Poll::Ready(Some(Err(error)));
                }
            }
        }

        match this
            .pending
            .as_mut()
            .unwrap()
            .as_mut()
            .poll(context)
        {
            Poll::Pending => Poll::Pending,
            Poll::Ready(result) => {
                this.pending = None;

                if !matches!(&result, Ok(Some(_))) {
                    this.phase = ReaderPhase::Done;
                }

                Poll::Ready(result.transpose())
            }
        }
    }
}

impl<B: Backend, T: FromGuest<B>> HasHandle<B> for Reader<B, T> {
    fn handle(&self) -> &Handle<B> {
        self.object.handle()
    }
}

impl<B: BackendValues, T: FromGuest<B>> ObjectProtocol<B> for Reader<B, T> {}

impl<B: Backend, T: FromGuest<B>> Describe for Reader<B, T> {
    fn describe(expected: &mut Expected) {
        expected.push("ReadableStreamDefaultReader");
    }
}

impl<B, T> FromGuest<B> for Reader<B, T>
where
    B: BackendValues,
    T: FromGuest<B> + 'static,
{
    type Owned = Self;

    fn from_guest<'js>(enter: &Enter<'js, B>, value: Val<'js, B>) -> Result<Self::Owned, Error> {
        let object = Object::from_guest(enter, value)?;

        object.function("read")?;
        object.function("cancel")?;
        object.function("releaseLock")?;

        Ok(Self {
            object,
            pending: None,
            phase: ReaderPhase::Open,
        })
    }
}

impl<B: Backend, T: FromGuest<B>> ToGuest<B> for Reader<B, T> {
    fn to_guest<'js>(self, enter: &Enter<'js, B>) -> Result<Val<'js, B>, Error> {
        self.object.to_guest(enter)
    }
}

impl<B: Backend, T: FromGuest<B>> ToGuest<B> for &Reader<B, T> {
    fn to_guest<'js>(self, enter: &Enter<'js, B>) -> Result<Val<'js, B>, Error> {
        (&self.object).to_guest(enter)
    }
}

stream_handle!(
    WritableStream<B, T>, "WritableStream",
    methods ["getWriter", "abort", "close"],
    properties []
);

impl<B: BackendValues, T: 'static> WritableStream<B, T> {
    pub fn locked(&self) -> Result<bool, Error> {
        self.object.get::<bool, _>("locked")
    }

    pub fn get_writer(&self) -> Result<Writer<B, T>, Error> {
        self.object
            .call_method::<_, _, Writer<B, T>>("getWriter", ())
    }
}

impl<B: BackendPromises, T: 'static> WritableStream<B, T> {
    pub fn abort<R>(&self, reason: R) -> Result<Promise<B, ()>, Error>
    where
        R: ToGuest<B>,
    {
        self.object
            .call_method::<_, _, StreamPromise<()>>("abort", (reason,))
    }

    pub fn close(&self) -> Result<Promise<B, ()>, Error> {
        self.object
            .call_method::<_, _, StreamPromise<()>>("close", ())
    }
}

pub struct Writer<B: Backend, T> {
    object: Object<B>,
    state: WriterState,
    chunk: PhantomData<fn(T)>,
}

impl<B: Backend, T> Unpin for Writer<B, T> {}

impl<B, T> Writer<B, T>
where
    B: BackendPromises + BackendExceptions,
{
    fn poll_close_operation(&mut self, context: &mut Context<'_>) -> Poll<Result<(), Error>> {
        match self.state.phase() {
            WriterPhase::Closed => return Poll::Ready(Ok(())),
            WriterPhase::Failed | WriterPhase::Released => {
                return Poll::Ready(Err(Error::unexpected("writer is not open")));
            }
            WriterPhase::Closing => return self.state.poll(context),
            WriterPhase::Writing => match self.state.poll(context) {
                Poll::Ready(Ok(())) => {}
                result => return result,
            },
            _ => {}
        }

        match self
            .object
            .call_method::<_, _, StreamPromise<()>>("close", ())
        {
            Ok(promise) => {
                self.state
                    .begin(WriterPhase::Closing, promise.into_future());

                self.state.poll(context)
            }
            Err(error) => {
                self.state.reset(WriterPhase::Failed);

                Poll::Ready(Err(error))
            }
        }
    }
}

impl<B: BackendValues, T> Writer<B, T> {
    pub fn desired_size(&self) -> Result<Option<f64>, Error> {
        self.object
            .get::<Option<f64>, _>("desiredSize")
    }

    pub fn release_lock(&mut self) -> Result<(), Error> {
        self.object
            .call_method::<_, _, ()>("releaseLock", ())?;

        self.state.reset(WriterPhase::Released);

        Ok(())
    }
}

impl<B: BackendPromises, T> Writer<B, T> {
    pub fn ready(&self) -> Result<Promise<B, ()>, Error> {
        self.object
            .get::<StreamPromise<()>, _>("ready")
    }

    pub fn closed(&self) -> Result<Promise<B, ()>, Error> {
        self.object
            .get::<StreamPromise<()>, _>("closed")
    }
}

impl<B, T> Writer<B, T>
where
    B: BackendPromises + BackendExceptions,
{
    pub async fn close(&mut self) -> Result<(), Error> {
        poll_fn(|context| self.poll_close_operation(context)).await
    }

    pub async fn abort<R>(&mut self, reason: R) -> Result<(), Error>
    where
        R: ToGuest<B>,
    {
        let promise = self
            .object
            .call_method::<_, _, StreamPromise<()>>("abort", (reason,))?;

        self.state.reset(WriterPhase::Failed);

        promise.await?;
        self.state.reset(WriterPhase::Closed);

        Ok(())
    }
}

impl<B, T> Writer<B, T>
where
    B: BackendPromises + BackendExceptions,
    T: ToGuest<B> + 'static,
{
    pub async fn write(&mut self, chunk: T) -> Result<(), Error> {
        self.send(chunk).await
    }
}

impl<B, T> Sink<T> for Writer<B, T>
where
    B: BackendPromises + BackendExceptions,
    T: ToGuest<B> + 'static,
{
    type Error = Error;

    fn poll_ready(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Result<(), Error>> {
        let this = self.get_mut();

        match this.state.phase() {
            WriterPhase::Open => Poll::Ready(Ok(())),
            WriterPhase::Writing => this.state.poll(context),
            _ => Poll::Ready(Err(Error::unexpected("writer is not open"))),
        }
    }

    fn start_send(self: Pin<&mut Self>, chunk: T) -> Result<(), Error> {
        let this = self.get_mut();

        if this.state.phase() != WriterPhase::Open {
            return Err(Error::unexpected("writer is not ready"));
        }

        match this
            .object
            .call_method::<_, _, StreamPromise<()>>("write", (chunk,))
        {
            Ok(promise) => {
                this.state
                    .begin(WriterPhase::Writing, promise.into_future());

                Ok(())
            }
            Err(error) => {
                this.state.reset(WriterPhase::Failed);

                Err(error)
            }
        }
    }

    fn poll_flush(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Result<(), Error>> {
        let this = self.get_mut();

        match this.state.phase() {
            WriterPhase::Writing | WriterPhase::Closing => this.state.poll(context),
            WriterPhase::Failed | WriterPhase::Released => {
                Poll::Ready(Err(Error::unexpected("writer is not open")))
            }
            _ => Poll::Ready(Ok(())),
        }
    }

    fn poll_close(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Result<(), Error>> {
        self.get_mut()
            .poll_close_operation(context)
    }
}

impl<B: Backend, T> HasHandle<B> for Writer<B, T> {
    fn handle(&self) -> &Handle<B> {
        self.object.handle()
    }
}

impl<B: BackendValues, T> ObjectProtocol<B> for Writer<B, T> {}

impl<B: Backend, T> Describe for Writer<B, T> {
    fn describe(expected: &mut Expected) {
        expected.push("WritableStreamDefaultWriter");
    }
}

impl<B: BackendValues, T: 'static> FromGuest<B> for Writer<B, T> {
    type Owned = Self;

    fn from_guest<'js>(enter: &Enter<'js, B>, value: Val<'js, B>) -> Result<Self::Owned, Error> {
        let object = Object::from_guest(enter, value)?;

        object.function("write")?;
        object.function("close")?;
        object.function("abort")?;
        object.function("releaseLock")?;

        Ok(Self {
            object,
            state: WriterState::new(),
            chunk: PhantomData,
        })
    }
}

impl<B: Backend, T> ToGuest<B> for Writer<B, T> {
    fn to_guest<'js>(self, enter: &Enter<'js, B>) -> Result<Val<'js, B>, Error> {
        self.object.to_guest(enter)
    }
}

impl<B: Backend, T> ToGuest<B> for &Writer<B, T> {
    fn to_guest<'js>(self, enter: &Enter<'js, B>) -> Result<Val<'js, B>, Error> {
        (&self.object).to_guest(enter)
    }
}

stream_handle!(
    TransformStream<B, I, O>, "TransformStream",
    methods [],
    properties [
        "readable" => ReadableStream<B, O>,
        "writable" => WritableStream<B, I>
    ]
);

impl<B: BackendValues, I: 'static, O: 'static> TransformStream<B, I, O> {
    pub fn readable(&self) -> Result<ReadableStream<B, O>, Error> {
        self.object
            .get::<ReadableStream<B, O>, _>("readable")
    }

    pub fn writable(&self) -> Result<WritableStream<B, I>, Error> {
        self.object
            .get::<WritableStream<B, I>, _>("writable")
    }
}

#[cfg(test)]
mod tests {
    use std::{
        future::{pending, ready},
        task::{Context, Poll},
    };

    use futures::{Sink, Stream, task::noop_waker_ref};

    use crate::{
        backend::{
            exceptions::BackendExceptions, promises::BackendPromises, tests::Stub,
            values::BackendValues,
        },
        errors::Error,
        handle::{
            protocol::ObjectProtocol,
            stream::{
                ReadableStream, Reader, TransformStream, WritableStream, Writer, WriterPhase,
                WriterState,
            },
        },
        marshal::describe::Expected,
        marshal::{FromGuest, ToGuest},
    };

    #[test]
    fn descriptions_use_standard_javascript_names() {
        assert_eq!(Expected::of::<ReadableStream<Stub, String>>().to_string(), "ReadableStream",);
        assert_eq!(
            Expected::of::<Reader<Stub, String>>().to_string(),
            "ReadableStreamDefaultReader",
        );
        assert_eq!(Expected::of::<WritableStream<Stub, String>>().to_string(), "WritableStream",);
        assert_eq!(
            Expected::of::<Writer<Stub, String>>().to_string(),
            "WritableStreamDefaultWriter",
        );
        assert_eq!(
            Expected::of::<TransformStream<Stub, String, String>>().to_string(),
            "TransformStream",
        );
    }

    #[test]
    fn writer_state_completes_each_operation_once() {
        let mut state = WriterState::new();
        let mut context = Context::from_waker(noop_waker_ref());

        state.begin(WriterPhase::Writing, Box::pin(ready(Ok(()))));
        assert!(state.poll(&mut context).is_ready());
        assert_eq!(state.phase(), WriterPhase::Open);

        state.begin(WriterPhase::Closing, Box::pin(ready(Ok(()))));
        assert!(state.poll(&mut context).is_ready());
        assert_eq!(state.phase(), WriterPhase::Closed);
        assert!(state.poll(&mut context).is_ready());
        assert_eq!(state.phase(), WriterPhase::Closed);
    }

    #[test]
    fn pending_writes_keep_the_writer_busy() {
        let mut state = WriterState::new();

        state.begin(WriterPhase::Writing, Box::pin(pending()));

        assert!(
            state
                .poll(&mut Context::from_waker(noop_waker_ref()))
                .is_pending()
        );
        assert_eq!(state.phase(), WriterPhase::Writing);
    }

    #[test]
    fn writer_failure_is_terminal() {
        let mut state = WriterState::new();

        state.begin(WriterPhase::Writing, Box::pin(ready(Err(Error::unexpected("write failed")))));

        assert!(matches!(
            state.poll(&mut Context::from_waker(noop_waker_ref())),
            Poll::Ready(Err(_)),
        ));
        assert_eq!(state.phase(), WriterPhase::Failed);
    }

    #[allow(dead_code)]
    fn capabilities<B: BackendPromises + BackendExceptions>() {
        fn object<B: BackendValues, T: ObjectProtocol<B>>() {}
        fn from<B: BackendValues, T: FromGuest<B>>() {}
        fn to<B: BackendValues, T: ToGuest<B>>() {}
        fn reader<T: Stream<Item = Result<String, Error>> + Unpin>() {}
        fn writer<T: Sink<String, Error = Error> + Unpin>() {}

        object::<B, ReadableStream<B, String>>();
        from::<B, ReadableStream<B, String>>();
        to::<B, ReadableStream<B, String>>();
        to::<B, &ReadableStream<B, String>>();

        object::<B, Reader<B, String>>();
        from::<B, Reader<B, String>>();
        to::<B, Reader<B, String>>();
        to::<B, &Reader<B, String>>();
        reader::<Reader<B, String>>();

        object::<B, WritableStream<B, String>>();
        from::<B, WritableStream<B, String>>();
        to::<B, WritableStream<B, String>>();
        to::<B, &WritableStream<B, String>>();

        object::<B, Writer<B, String>>();
        from::<B, Writer<B, String>>();
        to::<B, Writer<B, String>>();
        to::<B, &Writer<B, String>>();
        writer::<Writer<B, String>>();

        object::<B, TransformStream<B, String, String>>();
        from::<B, TransformStream<B, String, String>>();
        to::<B, TransformStream<B, String, String>>();
        to::<B, &TransformStream<B, String, String>>();
    }
}
