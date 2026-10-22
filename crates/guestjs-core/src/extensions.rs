use std::{
    any::{Any, TypeId},
    cell::RefCell,
    collections::HashMap,
    rc::Rc,
};

#[derive(Default)]
pub struct Extensions {
    slots: RefCell<HashMap<TypeId, Rc<dyn Any>>>,
}

impl Extensions {
    pub fn insert<T>(&self, value: T) -> Rc<T>
    where
        T: 'static,
    {
        let value = Rc::new(value);
        let _replaced = self
            .slots
            .borrow_mut()
            .insert(TypeId::of::<T>(), value.clone());

        value
    }

    pub fn get<T>(&self) -> Option<Rc<T>>
    where
        T: 'static,
    {
        self.slots
            .borrow()
            .get(&TypeId::of::<T>())
            .cloned()
            .and_then(|value| value.downcast::<T>().ok())
    }

    pub fn get_or_insert_with<T, F>(&self, f: F) -> Rc<T>
    where
        T: 'static,
        F: FnOnce() -> T,
    {
        self.get::<T>()
            .unwrap_or_else(|| self.insert(f()))
    }
}

#[cfg(test)]
mod tests {
    use std::rc::{Rc, Weak};

    use crate::extensions::Extensions;

    struct Probe(Weak<Extensions>);

    impl Drop for Probe {
        fn drop(&mut self) {
            if let Some(extensions) = self.0.upgrade() {
                extensions.get::<u8>();
            }
        }
    }

    #[test]
    fn slots_are_keyed_by_type() {
        let extensions = Extensions::default();

        extensions.insert(1u32);

        assert_eq!(extensions.get::<u32>().as_deref(), Some(&1));
        assert!(extensions.get::<u64>().is_none());
    }

    #[test]
    fn insert_replaces_and_get_or_insert_with_keeps() {
        let extensions = Extensions::default();

        extensions.insert(1u32);
        extensions.insert(2u32);

        assert_eq!(*extensions.get_or_insert_with(|| 3u32), 2);
        assert_eq!(*extensions.get_or_insert_with(|| "state".to_string()), "state",);
    }

    #[test]
    fn replaced_value_drops_after_the_borrow() {
        let extensions = Rc::new(Extensions::default());

        extensions.insert(Probe(Rc::downgrade(&extensions)));
        extensions.insert(Probe(Rc::downgrade(&extensions)));
    }
}
