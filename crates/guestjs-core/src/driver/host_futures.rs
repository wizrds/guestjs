use std::{
    cell::RefCell,
    future::Future,
    pin::Pin,
    rc::Rc,
};

use crate::{
    backend::{
        exceptions::BackendExceptions,
        promises::BackendPromises,
        Backend,
        Val,
    },
    enter::Enter,
    errors::Error,
    guest::Guest,
};

pub type PendingResult<B> = Box<
    dyn for<'js> FnOnce(&Enter<'js, B>) -> Result<Val<'js, B>, Error>,
>;

pub type HostFuture<B> = Pin<
    Box<dyn Future<Output = Result<PendingResult<B>, Error>>>,
>;

struct InFlight<B: Backend> {
    resolve: B::Owned,
    reject: B::Owned,
    future: HostFuture<B>,
}

impl<B: Backend> InFlight<B> {
    fn new(resolve: B::Owned, reject: B::Owned, future: HostFuture<B>) -> Self {
        Self {
            resolve,
            reject,
            future,
        }
    }

    fn poll(
        &mut self,
        context: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<PendingResult<B>, Error>> {
        self.future.as_mut().poll(context)
    }

    fn settle(self, outcome: Result<PendingResult<B>, Error>) -> Settled<B> {
        Settled::new(self.resolve, self.reject, outcome)
    }
}

struct Settled<B: Backend> {
    resolve: B::Owned,
    reject: B::Owned,
    outcome: Result<PendingResult<B>, Error>,
}

impl<B: Backend> Settled<B> {
    fn new(
        resolve: B::Owned,
        reject: B::Owned,
        outcome: Result<PendingResult<B>, Error>,
    ) -> Self {
        Self {
            resolve,
            reject,
            outcome,
        }
    }
}

impl<B> Settled<B>
where
    B: BackendPromises + BackendExceptions,
{
    pub fn publish<'js>(self, enter: &Enter<'js, B>) -> Result<(), Error> {
        let token = enter.token();
        let (resolver, argument) = match self
            .outcome
            .and_then(|result| result(enter))
        {
            Ok(value) => (&self.resolve, value),
            Err(error) => (&self.reject, B::exception(token, error)?),
        };

        B::call(
            token,
            &B::attach(token, resolver)?,
            &B::undefined(token),
            &[argument],
        )
        .map(|_| ())
    }
}

pub struct PendingHostFutures<B: Backend> {
    in_flight: RefCell<Vec<InFlight<B>>>,
    settled: RefCell<Vec<Settled<B>>>,
}

impl<B: Backend> PendingHostFutures<B> {
    pub fn new() -> Self {
        Self {
            in_flight: RefCell::default(),
            settled: RefCell::default(),
        }
    }

    pub fn of(guest: &Guest<B>) -> Rc<Self> {
        guest
            .extensions()
            .get_or_insert_with(Self::new)
    }

    pub fn is_empty(&self) -> bool {
        self.in_flight.borrow().is_empty() && self.settled.borrow().is_empty()
    }

    pub fn has_settled(&self) -> bool {
        !self.settled.borrow().is_empty()
    }

    pub fn poll(&self, context: &mut std::task::Context<'_>) {
        for mut entry in self.in_flight.take() {
            match entry.poll(context) {
                std::task::Poll::Pending => {
                    self.in_flight.borrow_mut().push(entry);
                }
                std::task::Poll::Ready(outcome) => {
                    self.settled.borrow_mut().push(entry.settle(outcome));
                }
            }
        }
    }
}

impl<B> PendingHostFutures<B>
where
    B: BackendPromises + BackendExceptions,
{
    pub fn register<'js>(
        &self,
        enter: &Enter<'js, B>,
        future: HostFuture<B>,
    ) -> Result<Val<'js, B>, Error> {
        let (promise, resolve, reject) = B::promise(enter.token())?;
    
        self.in_flight.borrow_mut().push(InFlight::new(
            B::detach(enter.token(), resolve),
            B::detach(enter.token(), reject),
            future,
        ));
    
        Ok(promise)
    }

    pub fn publish<'js>(
        &self,
        enter: &Enter<'js, B>,
    ) -> Result<(), Error> {
        self.settled
            .take()
            .into_iter()
            .try_for_each(|settled| settled.publish(enter))
    }
}

impl<B: Backend> Default for PendingHostFutures<B> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use crate::{
        backend::tests::Stub,
        driver::host_futures::PendingHostFutures,
        guest::Guest,
        runtime::Runtime,
    };

    #[test]
    fn a_guest_has_one_set_of_host_futures() {
        let runtime = Runtime::<Stub>::new(()).unwrap();
        let guest = Guest::new(&runtime).unwrap();

        assert!(Rc::ptr_eq(
            &PendingHostFutures::of(&guest),
            &PendingHostFutures::of(&guest),
        ));
    }

    #[test]
    fn new_host_futures_are_empty() {
        let host_futures = PendingHostFutures::<Stub>::new();

        assert!(host_futures.is_empty());
        assert!(!host_futures.has_settled());
    }
}
