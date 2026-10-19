use crate::{
    backend::{
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
            CallableProtocol,
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
        ToGuest,
    },
};

pub struct Function<B: Backend> {
    handle: Handle<B>,
}

impl<B: Backend> Clone for Function<B> {
    fn clone(&self) -> Self {
        Self {
            handle: self.handle.clone(),
        }
    }
}

impl<B: Backend> HasHandle<B> for Function<B> {
    fn handle(&self) -> &Handle<B> {
        &self.handle
    }
}

impl<B: BackendValues> ObjectProtocol<B> for Function<B> {}

impl<B: BackendValues> CallableProtocol<B> for Function<B> {}

impl<B: Backend> Describe for Function<B> {
    fn describe(expected: &mut Expected) {
        expected.push("function");
    }
}

impl<B: BackendValues> FromGuest<B> for Function<B> {
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

impl<B: Backend> ToGuest<B> for Function<B> {
    fn to_guest<'js>(
        self,
        enter: &Enter<'js, B>,
    ) -> Result<Val<'js, B>, Error> {
        self.handle.attach(enter)
    }
}

impl<B: Backend> ToGuest<B> for &Function<B> {
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
            function::Function,
            protocol::{
                CallableProtocol,
                ObjectProtocol,
            },
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
        fn callable<B: BackendValues, T: CallableProtocol<B>>() {}
        fn from<B: BackendValues, T: FromGuest<B>>() {}
        fn to<B: BackendValues, T: ToGuest<B>>() {}

        object::<B, Function<B>>();
        callable::<B, Function<B>>();
        from::<B, Function<B>>();
        to::<B, Function<B>>();
        to::<B, &Function<B>>();
    }

    #[test]
    fn description_names_a_function() {
        assert_eq!(
            Expected::of::<Function<Stub>>().to_string(),
            "function",
        );
    }
}
