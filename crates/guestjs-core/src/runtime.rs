use std::{
    cell::Cell,
    num::NonZeroU64,
    rc::Rc,
};

use crate::{
    backend::{
        interrupt::BackendInterrupt,
        Backend,
        Tok,
    },
    errors::Error,
    extensions::Extensions,
    guest::{
        registry::GuestRegistry,
        Guest,
        GuestId,
    },
};

struct RuntimeState<B: Backend> {
    registry: Rc<GuestRegistry<B>>,
    extensions: Extensions,
    next_id: Cell<NonZeroU64>,
    engine: B::Engine,
}

impl<B: Backend> RuntimeState<B> {
    pub fn new(config: B::Config) -> Result<Self, Error> {
        Ok(Self {
            registry: Rc::default(),
            extensions: Extensions::default(),
            next_id: Cell::new(NonZeroU64::MIN),
            engine: B::engine(config)?,
        })
    }

    pub fn engine(&self) -> &B::Engine {
        &self.engine
    }

    pub fn extensions(&self) -> &Extensions {
        &self.extensions
    }

    pub fn next_id(&self) -> GuestId {
        let id = self.next_id.get();

        self.next_id.set(
            id.checked_add(1)
                .expect("guest id counter overflowed"),
        );

        GuestId::new(id)
    }

    pub fn register(&self, guest: &Guest<B>) {
        self.registry.insert(guest);
    }

    pub fn unregister(&self, context_id: B::ContextId) {
        self.registry.remove(context_id);
    }

    pub fn find_guest<'js>(&self, token: Tok<'js, B>) -> Option<Guest<B>> {
        self.registry.find(B::context_id(token))
    }

    pub fn guests(&self) -> Vec<Guest<B>> {
        self.registry.guests()
    }

    pub fn install_policy_interrupt(&self)
    where
        B: BackendInterrupt,
    {
        let registry = self.registry.clone();

        B::set_interrupt_condition(
            &self.engine,
            Rc::new(move || {
                registry
                    .guests()
                    .iter()
                    .any(|guest| guest.activity().is_due())
            }),
        );
    }
}

pub struct Runtime<B: Backend> {
    state: Rc<RuntimeState<B>>,
}

impl<B: Backend> Clone for Runtime<B> {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
        }
    }
}

impl<B: Backend> Runtime<B> {
    pub fn new(config: B::Config) -> Result<Self, Error> {
        Ok(Self {
            state: Rc::new(RuntimeState::new(config)?),
        })
    }

    pub(crate) fn register(&self, guest: &Guest<B>) {
        self.state.register(guest);
    }

    pub(crate) fn unregister(&self, context_id: B::ContextId) {
        self.state.unregister(context_id);
    }

    pub fn engine(&self) -> &B::Engine {
        self.state.engine()
    }

    pub fn extensions(&self) -> &Extensions {
        self.state.extensions()
    }

    pub fn next_id(&self) -> GuestId {
        self.state.next_id()
    }

    pub fn find_guest<'js>(&self, token: Tok<'js, B>) -> Option<Guest<B>> {
        self.state.find_guest(token)
    }

    pub fn guests(&self) -> Vec<Guest<B>> {
        self.state.guests()
    }

    pub fn install_policy_interrupt(&self)
    where
        B: BackendInterrupt,
    {
        self.state.install_policy_interrupt();
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use crate::{
        backend::{
            tests::Stub,
            Backend,
        },
        guest::Guest,
        runtime::Runtime,
    };

    #[test]
    fn guest_ids_increase_from_one() {
        let runtime = Runtime::<Stub>::new(()).unwrap();

        assert_eq!(runtime.next_id().get(), 1);
        assert_eq!(
            Guest::new(&runtime)
                .unwrap()
                .id()
                .get(),
            2,
        );
    }

    #[test]
    fn find_guest_names_the_owner_of_a_token() {
        let runtime = Runtime::<Stub>::new(()).unwrap();
        let first = Guest::new(&runtime).unwrap();
        let second = Guest::new(&runtime).unwrap();
        let context_id = second.context_id();

        assert_eq!(
            first
                .enter(|enter| {
                    Ok(runtime
                        .find_guest(enter.token())
                        .map(|guest| guest.id()))
                })
                .unwrap(),
            Some(first.id()),
        );

        drop(second);

        assert!(Stub::enter(
            runtime.engine(),
            &context_id,
            |token| runtime.find_guest(token).is_none(),
        ));
    }

    #[test]
    fn guests_lists_the_runtimes_live_guests() {
        let runtime = Runtime::<Stub>::new(()).unwrap();
        let first = Guest::new(&runtime).unwrap();
        let second = Guest::new(&runtime).unwrap();

        second.close();

        assert_eq!(
            runtime
                .guests()
                .iter()
                .map(Guest::id)
                .collect::<HashSet<_>>(),
            HashSet::from([first.id(), second.id()]),
        );

        drop(first);

        assert_eq!(
            runtime
                .guests()
                .iter()
                .map(Guest::id)
                .collect::<Vec<_>>(),
            vec![second.id()],
        );
    }

    #[test]
    fn dropping_a_listed_guest_unregisters_it() {
        let runtime = Runtime::<Stub>::new(()).unwrap();
        let guest = Guest::new(&runtime).unwrap();
        let guests = runtime.guests();

        drop(guest);
        drop(guests);

        assert!(runtime.guests().is_empty());
    }
}
