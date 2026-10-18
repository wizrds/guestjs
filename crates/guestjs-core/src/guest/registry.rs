use std::{
    cell::RefCell,
    collections::HashMap,
};

use crate::{
    backend::Backend,
    guest::{
        Guest,
        WeakGuest,
    },
};

pub struct GuestRegistry<B: Backend> {
    guests: RefCell<HashMap<B::ContextId, WeakGuest<B>>>,
}

impl<B: Backend> Default for GuestRegistry<B> {
    fn default() -> Self {
        Self {
            guests: RefCell::default(),
        }
    }
}

impl<B: Backend> GuestRegistry<B> {
    pub fn insert(&self, guest: &Guest<B>) {
        self.guests
            .borrow_mut()
            .insert(guest.context_id(), guest.downgrade());
    }

    pub fn remove(&self, context_id: B::ContextId) {
        self.guests
            .borrow_mut()
            .remove(&context_id);
    }

    pub fn find(&self, context_id: B::ContextId) -> Option<Guest<B>> {
        self.guests
            .borrow()
            .get(&context_id)
            .and_then(WeakGuest::upgrade)
    }

    pub fn guests(&self) -> Vec<Guest<B>> {
        self.guests
            .borrow()
            .values()
            .filter_map(WeakGuest::upgrade)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        backend::tests::Stub,
        guest::{
            registry::GuestRegistry,
            Guest,
        },
        runtime::Runtime,
    };

    #[test]
    fn guests_skips_dropped_guests() {
        let runtime = Runtime::<Stub>::new(()).unwrap();
        let kept = Guest::new(&runtime).unwrap();
        let dropped = Guest::new(&runtime).unwrap();
        let registry = GuestRegistry::default();

        registry.insert(&kept);
        registry.insert(&dropped);

        drop(dropped);

        assert_eq!(
            registry
                .guests()
                .iter()
                .map(Guest::id)
                .collect::<Vec<_>>(),
            vec![kept.id()],
        );
    }
}
