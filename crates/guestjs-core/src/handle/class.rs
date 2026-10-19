use std::{
    any::TypeId,
    marker::PhantomData,
    ops::{
        Deref,
        DerefMut,
    },
};

use crate::{
    backend::{
        classes::BackendClasses,
        values::{
            BackendValues,
            Kind,
        },
        Backend,
        Val,
    },
    enter::Enter,
    errors::Error,
    handle::{
        base::Handle,
        protocol::{
            ConstructorProtocol,
            ObjectProtocol,
        },
        HasHandle,
    },
    marshal::{
        describe::{
            Describe,
            Expected,
        },
        FromGuest,
        FromGuestMut,
        FromGuestRef,
        ToGuest,
    },
};

pub struct Class<B: Backend> {
    handle: Handle<B>,
}

impl<B: Backend> Clone for Class<B> {
    fn clone(&self) -> Self {
        Self {
            handle: self.handle.clone(),
        }
    }
}

impl<B: Backend> HasHandle<B> for Class<B> {
    fn handle(&self) -> &Handle<B> {
        &self.handle
    }
}

impl<B: BackendValues> ObjectProtocol<B> for Class<B> {}

impl<B: BackendValues> ConstructorProtocol<B> for Class<B> {}

impl<B: Backend> Describe for Class<B> {
    fn describe(expected: &mut Expected) {
        expected.push("class");
    }
}

impl<B: BackendValues> FromGuest<B> for Class<B> {
    type Owned = Self;

    fn from_guest<'js>(
        enter: &Enter<'js, B>,
        value: Val<'js, B>,
    ) -> Result<Self::Owned, Error> {
        match B::kind(enter.token(), &value) {
            Kind::Function => Ok(Self {
                handle: Handle::from_value(enter, value),
            }),
            kind => Err(Expected::of::<Self>().mismatch(kind)),
        }
    }
}

impl<B: Backend> ToGuest<B> for Class<B> {
    fn to_guest<'js>(
        self,
        enter: &Enter<'js, B>,
    ) -> Result<Val<'js, B>, Error> {
        self.handle.attach(enter)
    }
}

impl<B: Backend> ToGuest<B> for &Class<B> {
    fn to_guest<'js>(
        self,
        enter: &Enter<'js, B>,
    ) -> Result<Val<'js, B>, Error> {
        self.handle.attach(enter)
    }
}

pub trait Payload: 'static {
    const NAME: &'static str;
}

impl<C: Payload> Describe for C {
    fn describe(expected: &mut Expected) {
        expected.push(C::NAME);
    }
}

impl<B: BackendClasses, C: Payload> FromGuest<B> for C {
    type Owned = Instance<B, C>;

    fn from_guest<'js>(
        enter: &Enter<'js, B>,
        value: Val<'js, B>,
    ) -> Result<Self::Owned, Error> {
        match B::payload_type(enter.token(), &value)
            == Some(TypeId::of::<C>())
        {
            true => Ok(Instance {
                handle: Handle::from_value(enter, value),
                payload: PhantomData,
            }),
            false => Err(
                Expected::of::<C>().mismatch(B::kind(enter.token(), &value)),
            ),
        }
    }
}

pub struct PayloadRef<'a, B: BackendClasses, C: Payload> {
    borrow: B::Ref<'a, C>,
}

impl<B: BackendClasses, C: Payload> Deref for PayloadRef<'_, B, C> {
    type Target = C;

    fn deref(&self) -> &C {
        &self.borrow
    }
}

impl<B: BackendClasses, C: Payload> FromGuestRef<B> for C {
    type Ref<'a> = PayloadRef<'a, B, C>;

    fn from_guest_ref<'js, 'a>(
        enter: &Enter<'js, B>,
        value: &'a Val<'js, B>,
    ) -> Result<Self::Ref<'a>, Error> {
        B::borrow::<C>(enter.token(), value)
            .map(|borrow| PayloadRef { borrow })
    }
}

pub struct PayloadMut<'a, B: BackendClasses, C: Payload> {
    borrow: B::RefMut<'a, C>,
}

impl<B: BackendClasses, C: Payload> Deref for PayloadMut<'_, B, C> {
    type Target = C;

    fn deref(&self) -> &C {
        &self.borrow
    }
}

impl<B: BackendClasses, C: Payload> DerefMut for PayloadMut<'_, B, C> {
    fn deref_mut(&mut self) -> &mut C {
        &mut self.borrow
    }
}

impl<B: BackendClasses, C: Payload> FromGuestMut<B> for C {
    type Mut<'a> = PayloadMut<'a, B, C>;

    fn from_guest_mut<'js, 'a>(
        enter: &Enter<'js, B>,
        value: &'a Val<'js, B>,
    ) -> Result<Self::Mut<'a>, Error> {
        B::borrow_mut::<C>(enter.token(), value)
            .map(|borrow| PayloadMut { borrow })
    }
}

pub struct Instance<B: Backend, C: Payload> {
    handle: Handle<B>,
    payload: PhantomData<fn() -> C>,
}

impl<B: BackendClasses, C: Payload> Instance<B, C> {
    pub fn with_payload<F, R>(&self, f: F) -> Result<R, Error>
    where
        F: FnOnce(&C) -> R,
    {
        self.handle.with_enter(|enter, value| {
            Ok(f(&*B::borrow::<C>(enter.token(), value)?))
        })
    }

    pub fn with_payload_mut<F, R>(&self, f: F) -> Result<R, Error>
    where
        F: FnOnce(&mut C) -> R,
    {
        self.handle.with_enter(|enter, value| {
            Ok(f(&mut *B::borrow_mut::<C>(enter.token(), value)?))
        })
    }
}

impl<B: Backend, C: Payload> Clone for Instance<B, C> {
    fn clone(&self) -> Self {
        Self {
            handle: self.handle.clone(),
            payload: PhantomData,
        }
    }
}

impl<B: Backend, C: Payload> HasHandle<B> for Instance<B, C> {
    fn handle(&self) -> &Handle<B> {
        &self.handle
    }
}

impl<B: BackendValues, C: Payload> ObjectProtocol<B> for Instance<B, C> {}

impl<B: Backend, C: Payload> ToGuest<B> for Instance<B, C> {
    fn to_guest<'js>(
        self,
        enter: &Enter<'js, B>,
    ) -> Result<Val<'js, B>, Error> {
        self.handle.attach(enter)
    }
}

impl<B: Backend, C: Payload> ToGuest<B> for &Instance<B, C> {
    fn to_guest<'js>(
        self,
        enter: &Enter<'js, B>,
    ) -> Result<Val<'js, B>, Error> {
        self.handle.attach(enter)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        backend::{
            classes::BackendClasses,
            tests::Stub,
            values::BackendValues,
        },
        handle::{
            class::{
                Class,
                Instance,
                Payload,
            },
            protocol::{
                ConstructorProtocol,
                ObjectProtocol,
            },
        },
        marshal::{
            describe::Expected,
            FromGuest,
            FromGuestMut,
            FromGuestRef,
            ToGuest,
        },
    };

    #[allow(dead_code)]
    struct Counter;

    impl Payload for Counter {
        const NAME: &'static str = "Counter";
    }

    #[allow(dead_code)]
    fn class_capabilities<B: BackendValues>() {
        fn object<B: BackendValues, T: ObjectProtocol<B>>() {}
        fn constructor<B: BackendValues, T: ConstructorProtocol<B>>() {}
        fn from<B: BackendValues, T: FromGuest<B>>() {}
        fn to<B: BackendValues, T: ToGuest<B>>() {}

        object::<B, Class<B>>();
        constructor::<B, Class<B>>();
        from::<B, Class<B>>();
        to::<B, Class<B>>();
        to::<B, &Class<B>>();
    }

    #[allow(dead_code)]
    fn payload_capabilities<B: BackendClasses>() {
        fn object<B: BackendClasses, T: ObjectProtocol<B>>() {}
        fn from<B, T>()
        where
            B: BackendClasses,
            T: FromGuest<B, Owned = Instance<B, Counter>>,
        {
        }
        fn from_ref<B: BackendClasses, T: FromGuestRef<B>>() {}
        fn from_mut<B: BackendClasses, T: FromGuestMut<B>>() {}
        fn to<B: BackendClasses, T: ToGuest<B>>() {}

        object::<B, Instance<B, Counter>>();
        from::<B, Counter>();
        from_ref::<B, Counter>();
        from_mut::<B, Counter>();
        to::<B, Instance<B, Counter>>();
        to::<B, &Instance<B, Counter>>();
    }

    #[test]
    fn class_description_names_a_class() {
        assert_eq!(Expected::of::<Class<Stub>>().to_string(), "class");
    }

    #[test]
    fn payload_description_names_the_payload() {
        assert_eq!(Expected::of::<Counter>().to_string(), "Counter");
    }
}
