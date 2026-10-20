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
        object::Object,
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
        ToGuestArgs,
    },
};

pub struct Class<B: Backend, R = Instance<B>> {
    handle: Handle<B>,
    marker: PhantomData<fn() -> R>,
}

impl<B: Backend, R> Class<B, R> {
    fn from_handle(handle: Handle<B>) -> Self {
        Self {
            handle,
            marker: PhantomData,
        }
    }

    pub fn with_result<O>(&self) -> Class<B, O> {
        Class::from_handle(self.handle.clone())
    }

    pub fn into_result<O>(self) -> Class<B, O> {
        Class::from_handle(self.handle)
    }
}

impl<B: BackendValues, R> Class<B, R> {
    pub fn construct<A>(&self, args: A) -> Result<R::Owned, Error>
    where
        A: ToGuestArgs<B>,
        R: FromGuest<B>,
    {
        self.construct_as::<A, R>(args)
    }

    pub fn construct_as<A, O>(
        &self,
        args: A,
    ) -> Result<O::Owned, Error>
    where
        A: ToGuestArgs<B>,
        O: FromGuest<B>,
    {
        ConstructorProtocol::construct::<O, A>(self, args)
    }
}

impl<B: Backend, R> Clone for Class<B, R> {
    fn clone(&self) -> Self {
        Self::from_handle(self.handle.clone())
    }
}

impl<B: Backend, R> HasHandle<B> for Class<B, R> {
    fn handle(&self) -> &Handle<B> {
        &self.handle
    }
}

impl<B: BackendValues, R> ObjectProtocol<B> for Class<B, R> {}

impl<B: BackendValues, R> ConstructorProtocol<B> for Class<B, R> {}

impl<B: Backend, R> Describe for Class<B, R> {
    fn describe(expected: &mut Expected) {
        expected.push("class");
    }
}

impl<B: BackendValues, R: 'static> FromGuest<B> for Class<B, R> {
    type Owned = Self;

    fn from_guest<'js>(
        enter: &Enter<'js, B>,
        value: Val<'js, B>,
    ) -> Result<Self::Owned, Error> {
        match B::kind(enter.token(), &value) {
            Kind::Function => {
                Ok(Self::from_handle(Handle::from_value(enter, value)))
            }
            kind => Err(Expected::of::<Self>().mismatch(kind)),
        }
    }
}

impl<B: Backend, R> ToGuest<B> for Class<B, R> {
    fn to_guest<'js>(
        self,
        enter: &Enter<'js, B>,
    ) -> Result<Val<'js, B>, Error> {
        self.handle.attach(enter)
    }
}

impl<B: Backend, R> ToGuest<B> for &Class<B, R> {
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
        Instance::<B, C>::from_guest(enter, value)
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

pub struct Instance<B: Backend, T = Object<B>> {
    object: Object<B>,
    marker: PhantomData<fn() -> T>,
}

impl<B: Backend, T> Instance<B, T> {
    fn from_object(object: Object<B>) -> Self {
        Self {
            object,
            marker: PhantomData,
        }
    }

    pub fn as_untyped(&self) -> Instance<B> {
        Instance::from_object(self.object.clone())
    }

    pub fn into_untyped(self) -> Instance<B> {
        Instance::from_object(self.object)
    }
}

impl<B: BackendClasses, T> Instance<B, T> {
    pub fn as_typed<C>(&self) -> Result<Instance<B, C>, Error>
    where
        C: Payload,
    {
        self.object.cast::<Instance<B, C>>()
    }

    pub fn into_typed<C>(self) -> Result<Instance<B, C>, Error>
    where
        C: Payload,
    {
        self.handle().with_enter(|enter, value| {
            Instance::<B, C>::check_payload(enter, value)
        })?;

        Ok(Instance::from_object(self.object))
    }

    pub fn borrow_as_with<C, F, R>(&self, f: F) -> Result<R, Error>
    where
        C: 'static,
        F: FnOnce(&C) -> R,
    {
        self.handle().with_enter(|enter, value| {
            Ok(f(&*B::borrow::<C>(enter.token(), value)?))
        })
    }

    pub fn borrow_as_with_mut<C, F, R>(&self, f: F) -> Result<R, Error>
    where
        C: 'static,
        F: FnOnce(&mut C) -> R,
    {
        self.handle().with_enter(|enter, value| {
            Ok(f(&mut *B::borrow_mut::<C>(enter.token(), value)?))
        })
    }
}

impl<B: BackendClasses, C: Payload> Instance<B, C> {
    fn check_payload<'js>(
        enter: &Enter<'js, B>,
        value: &Val<'js, B>,
    ) -> Result<(), Error> {
        match B::payload_type(enter.token(), value)
            == Some(TypeId::of::<C>())
        {
            true => Ok(()),
            false => Err(
                Expected::of::<C>().mismatch(B::kind(enter.token(), value)),
            ),
        }
    }

    pub fn borrow_with<F, R>(&self, f: F) -> Result<R, Error>
    where
        F: FnOnce(&C) -> R,
    {
        self.borrow_as_with::<C, F, R>(f)
    }

    pub fn borrow_with_mut<F, R>(&self, f: F) -> Result<R, Error>
    where
        F: FnOnce(&mut C) -> R,
    {
        self.borrow_as_with_mut::<C, F, R>(f)
    }
}

impl<B: Backend, T> Clone for Instance<B, T> {
    fn clone(&self) -> Self {
        Self::from_object(self.object.clone())
    }
}

impl<B: Backend, T> HasHandle<B> for Instance<B, T> {
    fn handle(&self) -> &Handle<B> {
        self.object.handle()
    }
}

impl<B: Backend, T> Deref for Instance<B, T> {
    type Target = Object<B>;

    fn deref(&self) -> &Self::Target {
        &self.object
    }
}

impl<B: BackendValues, T> ObjectProtocol<B> for Instance<B, T> {}

impl<B: Backend> Describe for Instance<B> {
    fn describe(expected: &mut Expected) {
        Object::<B>::describe(expected);
    }
}

impl<B: Backend, C: Payload> Describe for Instance<B, C> {
    fn describe(expected: &mut Expected) {
        C::describe(expected);
    }
}

impl<B: BackendValues> FromGuest<B> for Instance<B> {
    type Owned = Self;

    fn from_guest<'js>(
        enter: &Enter<'js, B>,
        value: Val<'js, B>,
    ) -> Result<Self::Owned, Error> {
        Object::from_guest(enter, value).map(Self::from_object)
    }
}

impl<B: BackendClasses, C: Payload> FromGuest<B> for Instance<B, C> {
    type Owned = Self;

    fn from_guest<'js>(
        enter: &Enter<'js, B>,
        value: Val<'js, B>,
    ) -> Result<Self::Owned, Error> {
        Self::check_payload(enter, &value)?;

        Object::from_guest(enter, value).map(Self::from_object)
    }
}

impl<B: Backend, T> ToGuest<B> for Instance<B, T> {
    fn to_guest<'js>(
        self,
        enter: &Enter<'js, B>,
    ) -> Result<Val<'js, B>, Error> {
        self.object.to_guest(enter)
    }
}

impl<B: Backend, T> ToGuest<B> for &Instance<B, T> {
    fn to_guest<'js>(
        self,
        enter: &Enter<'js, B>,
    ) -> Result<Val<'js, B>, Error> {
        (&self.object).to_guest(enter)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        any::TypeId,
        ops::Deref,
    };

    use crate::{
        backend::{
            classes::BackendClasses,
            tests::Stub,
            values::BackendValues,
            Backend,
        },
        errors::Error,
        handle::{
            class::{
                Class,
                Instance,
                Payload,
            },
            object::Object,
            protocol::{
                ConstructorProtocol,
                ObjectProtocol,
            },
            HasHandle,
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
    fn class_result_capabilities<B: BackendValues, R: 'static>() {
        fn handle<B: Backend, T: HasHandle<B> + Clone>() {}
        fn object<B: BackendValues, T: ObjectProtocol<B>>() {}
        fn constructor<B: BackendValues, T: ConstructorProtocol<B>>() {}
        fn from<B: BackendValues, T: FromGuest<B>>() {}
        fn to<B: Backend, T: ToGuest<B>>() {}

        handle::<B, Class<B, R>>();
        object::<B, Class<B, R>>();
        constructor::<B, Class<B, R>>();
        from::<B, Class<B, R>>();
        to::<B, Class<B, R>>();
        to::<B, &Class<B, R>>();
    }

    #[allow(dead_code)]
    fn class_result_methods<B, R>(
        class: &Class<B, R>,
    ) -> Result<R::Owned, Error>
    where
        B: BackendValues,
        R: FromGuest<B>,
    {
        class.with_result::<Object<B>>();
        class.clone().into_result::<Object<B>>();
        class.construct_as::<_, Object<B>>(())?;

        class.construct(())
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
        from::<B, Instance<B, Counter>>();
        from_ref::<B, Counter>();
        from_mut::<B, Counter>();
        to::<B, Instance<B, Counter>>();
        to::<B, &Instance<B, Counter>>();
    }

    #[allow(dead_code)]
    fn instance_capabilities<B: Backend, T>() {
        fn handle<B: Backend, T: HasHandle<B> + Clone>() {}
        fn to<B: Backend, T: ToGuest<B>>() {}
        fn object<B: Backend, T: Deref<Target = Object<B>>>() {}

        handle::<B, Instance<B, T>>();
        to::<B, Instance<B, T>>();
        to::<B, &Instance<B, T>>();
        object::<B, Instance<B, T>>();
    }

    #[allow(dead_code)]
    fn untyped_instance_capabilities<B: BackendValues>() {
        fn object<B: BackendValues, T: ObjectProtocol<B>>() {}
        fn from<B, T>()
        where
            B: BackendValues,
            T: FromGuest<B, Owned = Instance<B>>,
        {
        }

        object::<B, Instance<B>>();
        from::<B, Instance<B>>();
    }

    #[allow(dead_code)]
    fn instance_methods<B: BackendClasses>(
        instance: &Instance<B>,
        typed: &Instance<B, Counter>,
    ) -> Result<(), Error> {
        instance.as_untyped();
        instance.clone().into_untyped();
        instance.as_typed::<Counter>()?;
        instance.clone().into_typed::<Counter>()?;
        instance.borrow_as_with::<Counter, _, _>(|_| ())?;
        instance.borrow_as_with_mut::<Counter, _, _>(|_| ())?;
        typed.borrow_with(|_| ())?;
        typed.borrow_with_mut(|_| ())?;
        instance.cast::<Instance<B>>()?;
        instance.cast::<Instance<B, Counter>>()?;
        instance.cast::<Counter>()?;

        Ok(())
    }

    #[test]
    fn class_description_names_a_class() {
        assert_eq!(Expected::of::<Class<Stub>>().to_string(), "class");
    }

    #[test]
    fn class_defaults_to_the_untyped_instance_result() {
        assert_eq!(
            TypeId::of::<Class<Stub>>(),
            TypeId::of::<Class<Stub, Instance<Stub>>>(),
        );
    }

    #[test]
    fn class_description_does_not_depend_on_the_result() {
        assert_eq!(
            Expected::of::<Class<Stub, Counter>>().to_string(),
            "class",
        );
    }

    #[test]
    fn payload_description_names_the_payload() {
        assert_eq!(Expected::of::<Counter>().to_string(), "Counter");
    }

    #[test]
    fn untyped_instance_defaults_to_the_object_marker() {
        assert_eq!(
            TypeId::of::<Instance<Stub>>(),
            TypeId::of::<Instance<Stub, Object<Stub>>>(),
        );
    }

    #[test]
    fn untyped_instance_description_names_an_object() {
        assert_eq!(
            Expected::of::<Instance<Stub>>().to_string(),
            "object",
        );
    }

    #[test]
    fn typed_instance_description_names_the_payload() {
        assert_eq!(
            Expected::of::<Instance<Stub, Counter>>().to_string(),
            "Counter",
        );
    }
}
