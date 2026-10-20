use std::future::Future;

use crate::{
    backend::{
        callables::BackendCallables,
        classes::BackendClasses,
        exceptions::BackendExceptions,
        promises::BackendPromises,
        values::{
            Property,
            WellKnown,
        },
        Backend,
        Val,
    },
    enter::Enter,
    errors::Error,
    host::{
        class::HostClass,
        context::CallContext,
        declaration::Declaration,
        exception::{
            ExceptionDeclaration,
            HostException,
        },
        function::FunctionDeclaration,
        property::{
            PropertyDeclaration,
            PropertyKey,
        },
        state::HostState,
        value::ValueDeclaration,
    },
    marshal::{
        args::Args,
        ToGuest,
    },
};

pub struct NamespaceDeclaration<B: Backend> {
    properties: Vec<(PropertyKey, PropertyDeclaration<B>)>,
}

impl<B: Backend> NamespaceDeclaration<B> {
    pub fn new() -> Self {
        Self {
            properties: Vec::new(),
        }
    }

    pub fn property<K>(
        &mut self,
        key: K,
        property: PropertyDeclaration<B>,
    ) -> &mut Self
    where
        K: Into<PropertyKey>,
    {
        let key = key.into();

        match self
            .properties
            .iter_mut()
            .find(|(existing, _)| *existing == key)
        {
            Some((_, existing)) => *existing = existing.merge(property),
            None => self.properties.push((key, property)),
        }

        self
    }

    pub fn value<K, T>(&mut self, key: K, value: T) -> &mut Self
    where
        K: Into<PropertyKey>,
        T: ToGuest<B> + Clone + 'static,
    {
        self.property(key, PropertyDeclaration::value(ValueDeclaration::new(value)))
    }

    pub fn constant<K, T>(&mut self, key: K, value: T) -> &mut Self
    where
        K: Into<PropertyKey>,
        T: ToGuest<B> + Clone + 'static,
    {
        self.property(key, PropertyDeclaration::constant(ValueDeclaration::new(value)))
    }
}

impl<B: BackendCallables> NamespaceDeclaration<B> {
    pub fn function<K, F, R>(&mut self, key: K, function: F) -> &mut Self
    where
        K: Into<PropertyKey>,
        F: for<'e, 'js> Fn(
                &CallContext<'e, 'js, B>,
                &Args<'js, B>,
            ) -> Result<R, Error>
            + 'static,
        R: ToGuest<B>,
    {
        self.property(
            key,
            PropertyDeclaration::value(FunctionDeclaration::new(function)),
        )
    }

    pub fn getter<K, F, R>(&mut self, key: K, get: F) -> &mut Self
    where
        K: Into<PropertyKey>,
        F: for<'e, 'js> Fn(
                &CallContext<'e, 'js, B>,
                &Args<'js, B>,
            ) -> Result<R, Error>
            + 'static,
        R: ToGuest<B>,
    {
        self.property(
            key,
            PropertyDeclaration::getter(FunctionDeclaration::new(get)),
        )
    }

    pub fn setter<K, F, R>(&mut self, key: K, set: F) -> &mut Self
    where
        K: Into<PropertyKey>,
        F: for<'e, 'js> Fn(
                &CallContext<'e, 'js, B>,
                &Args<'js, B>,
            ) -> Result<R, Error>
            + 'static,
        R: ToGuest<B>,
    {
        self.property(
            key,
            PropertyDeclaration::setter(FunctionDeclaration::new(set)),
        )
    }

    pub fn namespace<K>(
        &mut self,
        key: K,
        namespace: NamespaceDeclaration<B>,
    ) -> &mut Self
    where
        K: Into<PropertyKey>,
    {
        self.property(key, PropertyDeclaration::value(namespace))
    }

    pub fn install<'js>(
        &self,
        enter: &Enter<'js, B>,
        state: &HostState,
        target: &Val<'js, B>,
    ) -> Result<(), Error> {
        self.properties
            .iter()
            .map(|(key, property)| {
                Ok((key.to_guest(enter)?, property.realise(enter, state, key)?))
            })
            .collect::<Result<Vec<_>, Error>>()?
            .into_iter()
            .try_for_each(|(key, property)| {
                B::define(enter.token(), target, &key, property)
            })
    }
}

impl<B> NamespaceDeclaration<B>
where
    B: BackendCallables + BackendPromises + BackendExceptions,
{
    pub fn function_async<K, F, Fut, R>(&mut self, key: K, function: F) -> &mut Self
    where
        K: Into<PropertyKey>,
        F: for<'e, 'js> Fn(
                &CallContext<'e, 'js, B>,
                &Args<'js, B>,
            ) -> Result<Fut, Error>
            + 'static,
        Fut: Future<Output = Result<R, Error>> + 'static,
        R: ToGuest<B> + 'static,
    {
        self.property(
            key,
            PropertyDeclaration::value(FunctionDeclaration::new_async(function)),
        )
    }
}

impl<B> NamespaceDeclaration<B>
where
    B: BackendClasses + BackendExceptions,
{
    pub fn class<C>(&mut self) -> &mut Self
    where
        C: HostClass<B>,
    {
        self.property(C::NAME, PropertyDeclaration::value(C::declaration()))
    }

    pub fn exception<E>(&mut self) -> &mut Self
    where
        E: HostException,
    {
        self.property(
            E::NAME,
            PropertyDeclaration::value(ExceptionDeclaration::<E>::new()),
        )
    }
}

impl<B: Backend> Default for NamespaceDeclaration<B> {
    fn default() -> Self {
        Self::new()
    }
}

impl<B: Backend> Clone for NamespaceDeclaration<B> {
    fn clone(&self) -> Self {
        Self {
            properties: self.properties.clone(),
        }
    }
}

impl<B: BackendCallables> Declaration<B> for NamespaceDeclaration<B> {
    fn realise<'js>(
        &self,
        enter: &Enter<'js, B>,
        state: &HostState,
        name: &str,
    ) -> Result<Val<'js, B>, Error> {
        let object = B::object(enter.token())?;

        B::define(
            enter.token(),
            &object,
            &B::well_known(enter.token(), WellKnown::ToStringTag),
            Property::Value {
                value: name.to_guest(enter)?,
                writable: false,
                enumerable: false,
                configurable: true,
            },
        )?;

        self.install(enter, state, &object)?;

        Ok(object)
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use crate::{
        backend::{
            callables::BackendCallables,
            classes::BackendClasses,
            exceptions::BackendExceptions,
            promises::BackendPromises,
            values::WellKnown,
        },
        errors::Error,
        handle::class::Payload,
        host::{
            class::{
                ClassDeclaration,
                HostClass,
            },
            declaration::Declaration,
            exception::HostException,
            namespace::NamespaceDeclaration,
            property::PropertyDeclaration,
            value::ValueDeclaration,
        },
    };

    struct Counter;

    impl Payload for Counter {
        const NAME: &'static str = "Counter";
    }

    impl<B> HostClass<B> for Counter
    where
        B: BackendClasses + BackendExceptions,
    {
        fn declaration() -> ClassDeclaration<B, Self> {
            ClassDeclaration::new()
        }
    }

    struct NetworkError;

    impl HostException for NetworkError {
        const NAME: &'static str = "NetworkError";
    }

    #[allow(dead_code)]
    fn capabilities<B>()
    where
        B: BackendClasses + BackendPromises + BackendExceptions,
    {
        fn declaration<B: BackendCallables, T: Declaration<B>>() {}
        fn clone<T: Clone>() {}

        let _ = NamespaceDeclaration::<B>::new()
            .value("version", 1.0)
            .constant("MAX", 10.0)
            .function("add", |_, _| Ok::<_, Error>(1.0))
            .function(WellKnown::Iterator, |_, _| Ok::<_, Error>(()))
            .function_async("fetch", |_, _| {
                Ok::<_, Error>(async { Ok::<_, Error>(1.0) })
            })
            .getter("now", |_, _| Ok::<_, Error>(1.0))
            .setter("now", |_, _| Ok::<_, Error>(()))
            .class::<Counter>()
            .exception::<NetworkError>()
            .namespace("nested", NamespaceDeclaration::new())
            .property(
                "raw",
                PropertyDeclaration::Value {
                    declaration: Rc::new(ValueDeclaration::new(1.0)),
                    writable: false,
                    enumerable: true,
                    configurable: false,
                },
            );

        declaration::<B, NamespaceDeclaration<B>>();
        clone::<NamespaceDeclaration<B>>();
    }
}
