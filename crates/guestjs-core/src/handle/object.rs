use crate::{
    backend::{
        values::BackendValues,
        Backend,
        Val,
    },
    enter::Enter,
    errors::Error,
    handle::{
        base::Handle,
        protocol::ObjectProtocol,
        HasHandle,
    },
    marshal::{
        describe::{
            Describe,
            Expected,
        },
        FromGuest,
        ToGuest,
    },
};

pub struct Object<B: Backend> {
    handle: Handle<B>,
}

impl<B: Backend> Clone for Object<B> {
    fn clone(&self) -> Self {
        Self {
            handle: self.handle.clone(),
        }
    }
}

impl<B: Backend> HasHandle<B> for Object<B> {
    fn handle(&self) -> &Handle<B> {
        &self.handle
    }
}

impl<B: BackendValues> ObjectProtocol<B> for Object<B> {}

impl<B: Backend> Describe for Object<B> {
    fn describe(expected: &mut Expected) {
        expected.push("object");
    }
}

impl<B: BackendValues> FromGuest<B> for Object<B> {
    type Owned = Self;

    fn from_guest<'js>(
        enter: &Enter<'js, B>,
        value: Val<'js, B>,
    ) -> Result<Self::Owned, Error> {
        match B::kind(enter.token(), &value) {
            kind if kind.is_object() => Ok(Self {
                handle: Handle::from_value(enter, value),
            }),
            kind => Err(Expected::of::<Self>().mismatch(kind)),
        }
    }
}

impl<B: Backend> ToGuest<B> for Object<B> {
    fn to_guest<'js>(
        self,
        enter: &Enter<'js, B>,
    ) -> Result<Val<'js, B>, Error> {
        self.handle.attach(enter)
    }
}

impl<B: Backend> ToGuest<B> for &Object<B> {
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
            tests::Stub,
            values::BackendValues,
        },
        handle::{
            object::Object,
            protocol::ObjectProtocol,
        },
        marshal::{
            describe::Expected,
            FromGuest,
            ToGuest,
        },
    };

    #[allow(dead_code)]
    fn capabilities<B: BackendValues>() {
        fn object<B: BackendValues, T: ObjectProtocol<B>>() {}
        fn from<B: BackendValues, T: FromGuest<B>>() {}
        fn to<B: BackendValues, T: ToGuest<B>>() {}

        object::<B, Object<B>>();
        from::<B, Object<B>>();
        to::<B, Object<B>>();
        to::<B, &Object<B>>();
    }

    #[test]
    fn description_names_an_object() {
        assert_eq!(Expected::of::<Object<Stub>>().to_string(), "object");
    }
}
