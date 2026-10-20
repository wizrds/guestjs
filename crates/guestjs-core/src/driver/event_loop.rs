use crate::{
    backend::{
        exceptions::BackendExceptions,
        promises::BackendPromises,
        Backend,
    },
    driver::host_futures::PendingHostFutures,
    errors::Error,
    guest::Guest,
};

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Progress {
    Ready,
    Waiting,
    Idle,
}

pub struct EventLoop<B: Backend> {
    guest: Guest<B>,
}

impl<B> EventLoop<B>
where
    B: BackendPromises + BackendExceptions,
{
    pub fn new(guest: &Guest<B>) -> Self {
        Self {
            guest: guest.clone(),
        }
    }

    pub fn guest(&self) -> &Guest<B> {
        &self.guest
    }

    pub fn step(
        &self,
        context: &mut std::task::Context<'_>,
    ) -> Result<Progress, Error> {
        let host_futures = PendingHostFutures::of(&self.guest);

        host_futures.poll(context);

        let published = host_futures.has_settled();

        if published {
            self.guest.enter(|enter| host_futures.publish(enter))?;
        }

        Ok(match self.guest.activate(|| {
            match B::poll_jobs(self.guest.runtime().engine(), context) {
                std::task::Poll::Ready(result) => result.map(std::task::Poll::Ready),
                std::task::Poll::Pending => Ok(std::task::Poll::Pending),
            }
        })? {
            std::task::Poll::Ready(true) => Progress::Ready,
            _ if published => Progress::Ready,
            std::task::Poll::Ready(false) if host_futures.is_empty() => Progress::Idle,
            std::task::Poll::Ready(false) | std::task::Poll::Pending => Progress::Waiting,
        })
    }

    pub async fn run_until_idle(&self) -> Result<(), Error> {
        let armed = self
            .guest
            .activity()
            .policy()
            .arm();

        std::future::poll_fn(|context| match armed.due_error() {
            Some(error) => std::task::Poll::Ready(Err(error)),
            None => match self.step(context) {
                Ok(Progress::Idle) => std::task::Poll::Ready(Ok(())),
                Ok(Progress::Ready) => {
                    context.waker().wake_by_ref();

                    std::task::Poll::Pending
                },
                Ok(Progress::Waiting) => std::task::Poll::Pending,
                Err(error) => std::task::Poll::Ready(Err(error)),
            },
        })
        .await
    }
}
