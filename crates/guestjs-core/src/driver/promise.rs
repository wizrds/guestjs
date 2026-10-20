use std::{
    future::Future,
    pin::Pin,
};

use crate::{
    backend::{
        exceptions::BackendExceptions,
        promises::{
            BackendPromises,
            PromiseState,
        },
        Backend,
        Val,
    },
    driver::event_loop::{
        EventLoop,
        Progress,
    },
    enter::Enter,
    errors::Error,
    policy::ArmedPolicy,
};

pub struct PromiseFuture<B: Backend> {
    event_loop: EventLoop<B>,
    promise: B::Owned,
    armed: Option<ArmedPolicy>,
}

impl<B: Backend> Unpin for PromiseFuture<B> {}

impl<B> PromiseFuture<B>
where
    B: BackendPromises + BackendExceptions,
{
    pub fn new<'js>(
        enter: &Enter<'js, B>,
        value: Val<'js, B>,
    ) -> Result<Self, Error> {
        Ok(Self {
            event_loop: EventLoop::new(enter.guest()),
            promise: B::detach(
                enter.token(),
                B::resolve(enter.token(), value)?,
            ),
            armed: None,
        })
    }

    pub fn settled(&self) -> Result<Option<B::Owned>, Error> {
        self.event_loop
            .guest()
            .enter(|enter| {
                match B::state(
                    enter.token(),
                    &B::attach(enter.token(), &self.promise)?,
                )? {
                    PromiseState::Pending => Ok(None),
                    PromiseState::Fulfilled(value) => {
                        Ok(Some(B::detach(enter.token(), value)))
                    }
                    PromiseState::Rejected(reason) => {
                        Err(B::throw(enter.token(), reason))
                    }
                }
            })
    }
}

impl<B> Future for PromiseFuture<B>
where
    B: BackendPromises + BackendExceptions,
{
    type Output = Result<B::Owned, Error>;

    fn poll(
        self: Pin<&mut Self>,
        context: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let this = self.get_mut();

        if let Some(error) = this
            .armed
            .get_or_insert_with(|| {
                this.event_loop
                    .guest()
                    .activity()
                    .policy()
                    .arm()
            })
            .due_error()
        {
            return std::task::Poll::Ready(Err(error));
        }

        match this.settled() {
            Ok(None) => match this.event_loop.step(context) {
                Ok(Progress::Ready) => {
                    context.waker().wake_by_ref();

                    std::task::Poll::Pending
                }
                Ok(Progress::Waiting | Progress::Idle) => std::task::Poll::Pending,
                Err(error) => std::task::Poll::Ready(Err(error)),
            },
            Ok(Some(value)) => std::task::Poll::Ready(Ok(value)),
            Err(error) => std::task::Poll::Ready(Err(error)),
        }
    }
}
