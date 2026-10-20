use std::{
    any::TypeId,
    cell::RefCell,
    collections::HashMap,
    future::Future,
    rc::Rc,
};

use crate::{
    backend::{
        callables::BackendCallables,
        classes::BackendClasses,
        exceptions::BackendExceptions,
        promises::BackendPromises,
        values::{
            BackendValues,
            Property,
        },
        Backend,
        Val,
    },
    enter::Enter,
    errors::{
        Error,
        GuestErrorKind,
    },
    guest::Guest,
    handle::class::{
        Class,
        Instance,
        Payload,
    },
    host::{
        context::{
            CallContext,
            FromContext,
        },
        declaration::Declaration,
        namespace::NamespaceDeclaration,
        property::{
            PropertyDeclaration,
            PropertyKey,
        },
        requirements::Requirements,
        state::HostState,
    },
    marshal::{
        args::Args,
        FromGuest,
        ToGuest,
    },
};

pub struct ClassPrototype<B: Backend> {
    prototype: B::Owned,
}

impl<B: Backend> ClassPrototype<B> {
    pub fn new<'js>(enter: &Enter<'js, B>, prototype: Val<'js, B>) -> Self {
        Self {
            prototype: B::detach(enter.token(), prototype),
        }
    }

    pub fn get<'js>(&self, enter: &Enter<'js, B>) -> Result<Val<'js, B>, Error> {
        B::attach(enter.token(), &self.prototype)
    }
}

impl<B: BackendValues> ClassPrototype<B> {
    pub fn for_target<'js>(
        &self,
        enter: &Enter<'js, B>,
        new_target: Option<&Val<'js, B>>,
    ) -> Result<Val<'js, B>, Error> {
        match new_target {
            Some(new_target) => B::get(
                enter.token(),
                new_target,
                &"prototype".to_guest(enter)?,
            )
            .and_then(|prototype| {
                if B::kind(enter.token(), &prototype).is_object() {
                    Ok(prototype)
                } else {
                    self.get(enter)
                }
            }),
            None => self.get(enter),
        }
    }
}

impl<B: Backend> Clone for ClassPrototype<B> {
    fn clone(&self) -> Self {
        Self {
            prototype: self.prototype.clone(),
        }
    }
}

pub struct RealisedClass<B: Backend> {
    constructor: B::Owned,
    prototype: ClassPrototype<B>,
}

impl<B: Backend> RealisedClass<B> {
    pub fn new<'js>(
        enter: &Enter<'js, B>,
        constructor: Val<'js, B>,
        prototype: ClassPrototype<B>,
    ) -> Self {
        Self {
            constructor: B::detach(enter.token(), constructor),
            prototype,
        }
    }

    pub fn link<'js>(
        enter: &Enter<'js, B>,
        constructor: Val<'js, B>,
        prototype: Val<'js, B>,
    ) -> Result<Self, Error>
    where
        B: BackendValues,
    {
        B::define(
            enter.token(),
            &constructor,
            &"prototype".to_guest(enter)?,
            Property::Value {
                value: prototype.clone(),
                writable: false,
                enumerable: false,
                configurable: false,
            },
        )?;

        B::define(
            enter.token(),
            &prototype,
            &"constructor".to_guest(enter)?,
            Property::Value {
                value: constructor.clone(),
                writable: true,
                enumerable: false,
                configurable: true,
            },
        )?;

        Ok(Self::new(
            enter,
            constructor,
            ClassPrototype::new(enter, prototype),
        ))
    }

    pub fn constructor<'js>(&self, enter: &Enter<'js, B>) -> Result<Val<'js, B>, Error> {
        B::attach(enter.token(), &self.constructor)
    }

    pub fn prototype(&self) -> &ClassPrototype<B> {
        &self.prototype
    }
}

impl<B: Backend> Clone for RealisedClass<B> {
    fn clone(&self) -> Self {
        Self {
            constructor: self.constructor.clone(),
            prototype: self.prototype.clone(),
        }
    }
}

pub struct RealisedClasses<B: Backend> {
    classes: RefCell<HashMap<TypeId, RealisedClass<B>>>,
}

impl<B: Backend> RealisedClasses<B> {
    pub fn new() -> Self {
        Self {
            classes: RefCell::new(HashMap::new()),
        }
    }

    pub fn of(guest: &Guest<B>) -> Rc<Self> {
        guest
            .extensions()
            .get_or_insert_with(Self::new)
    }

    pub fn get<T: 'static>(&self) -> Option<RealisedClass<B>> {
        self.classes
            .borrow()
            .get(&TypeId::of::<T>())
            .cloned()
    }

    pub fn insert<T: 'static>(&self, class: RealisedClass<B>) -> RealisedClass<B> {
        self.classes
            .borrow_mut()
            .insert(TypeId::of::<T>(), class.clone());

        class
    }

    pub fn get_or_realise<T, F>(&self, realise: F) -> Result<RealisedClass<B>, Error>
    where
        T: 'static,
        F: FnOnce() -> Result<RealisedClass<B>, Error>,
    {
        match self.get::<T>() {
            Some(class) => Ok(class),
            None => realise().map(|class| self.insert::<T>(class)),
        }
    }
}

impl<B: Backend> Default for RealisedClasses<B> {
    fn default() -> Self {
        Self::new()
    }
}

pub trait HostClass<B: Backend>: Payload + Sized {
    fn declaration() -> ClassDeclaration<B, Self>;

    fn class<'js>(
        enter: &Enter<'js, B>,
    ) -> Result<Class<B, Instance<B, Self>>, Error>
    where
        B: BackendClasses + BackendExceptions,
    {
        Class::from_guest(
            enter,
            Self::declaration()
                .realised(enter, &HostState::empty())?
                .constructor(enter)?,
        )
    }
}

impl<B, C> ToGuest<B> for C
where
    B: BackendClasses,
    C: Payload,
{
    fn to_guest<'js>(
        self,
        enter: &Enter<'js, B>,
    ) -> Result<Val<'js, B>, Error> {
        B::instantiate(
            enter.token(),
            &RealisedClasses::of(enter.guest())
                .get::<C>()
                .ok_or_else(|| {
                    Error::unexpected(format!(
                        "host class {} is not realised in this guest",
                        C::NAME,
                    ))
                })?
                .prototype()
                .get(enter)?,
            self,
        )
    }
}

pub struct ClassDeclaration<B: Backend, C: Payload> {
    constructor: Option<
        Rc<
            dyn for<'e, 'js> Fn(
                &CallContext<'e, 'js, B>,
                &Args<'js, B>,
            ) -> Result<C, Error>,
        >,
    >,
    prototype: NamespaceDeclaration<B>,
    statics: NamespaceDeclaration<B>,
    requirements: Requirements,
}

impl<B: Backend, C: Payload> ClassDeclaration<B, C> {
    pub fn new() -> Self {
        Self {
            constructor: None,
            prototype: NamespaceDeclaration::new(),
            statics: NamespaceDeclaration::new(),
            requirements: Requirements::new(),
        }
    }

    pub fn require<T>(mut self) -> Self
    where
        T: FromContext<B>,
    {
        T::declare(&mut self.requirements);

        self
    }

    pub fn constructor<F>(mut self, constructor: F) -> Self
    where
        F: for<'e, 'js> Fn(
                &CallContext<'e, 'js, B>,
                &Args<'js, B>,
            ) -> Result<C, Error>
            + 'static,
    {
        self.constructor = Some(Rc::new(constructor));

        self
    }

    pub fn property<K>(mut self, key: K, property: PropertyDeclaration<B>) -> Self
    where
        K: Into<PropertyKey>,
    {
        self.prototype.property(key, property);

        self
    }

    pub fn value<K, T>(mut self, key: K, value: T) -> Self
    where
        K: Into<PropertyKey>,
        T: ToGuest<B> + Clone + 'static,
    {
        self.prototype.value(key, value);

        self
    }

    pub fn constant<K, T>(mut self, key: K, value: T) -> Self
    where
        K: Into<PropertyKey>,
        T: ToGuest<B> + Clone + 'static,
    {
        self.prototype.constant(key, value);

        self
    }

    pub fn statics(mut self, statics: NamespaceDeclaration<B>) -> Self {
        self.statics = statics;

        self
    }
}

impl<B: BackendCallables, C: Payload> ClassDeclaration<B, C> {
    pub fn method<K, F, R>(mut self, key: K, method: F) -> Self
    where
        K: Into<PropertyKey>,
        F: for<'e, 'js> Fn(
                &CallContext<'e, 'js, B>,
                &Args<'js, B>,
            ) -> Result<R, Error>
            + 'static,
        R: ToGuest<B>,
    {
        self.prototype.function(key, method);

        self
    }

    pub fn getter<K, F, R>(mut self, key: K, get: F) -> Self
    where
        K: Into<PropertyKey>,
        F: for<'e, 'js> Fn(
                &CallContext<'e, 'js, B>,
                &Args<'js, B>,
            ) -> Result<R, Error>
            + 'static,
        R: ToGuest<B>,
    {
        self.prototype.getter(key, get);

        self
    }

    pub fn setter<K, F, R>(mut self, key: K, set: F) -> Self
    where
        K: Into<PropertyKey>,
        F: for<'e, 'js> Fn(
                &CallContext<'e, 'js, B>,
                &Args<'js, B>,
            ) -> Result<R, Error>
            + 'static,
        R: ToGuest<B>,
    {
        self.prototype.setter(key, set);

        self
    }
}

impl<B, C> ClassDeclaration<B, C>
where
    B: BackendCallables + BackendPromises + BackendExceptions,
    C: Payload,
{
    pub fn method_async<K, F, Fut, R>(mut self, key: K, method: F) -> Self
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
        self.prototype.function_async(key, method);

        self
    }
}

impl<B, C> ClassDeclaration<B, C>
where
    B: BackendClasses + BackendExceptions,
    C: Payload,
{
    pub fn realised<'js>(
        &self,
        enter: &Enter<'js, B>,
        state: &HostState,
    ) -> Result<RealisedClass<B>, Error> {
        self.requirements.check(state)?;

        RealisedClasses::of(enter.guest()).get_or_realise::<C, _>(|| {
            let prototype = B::new_object(enter.token())?;

            self.prototype.install(enter, state, &prototype)?;

            let constructor = B::constructor(
                enter.token(),
                C::NAME,
                0,
                enter.guest().raw_body({
                    let construct = self.constructor.clone();
                    let state = state.clone();
                    let prototype = ClassPrototype::new(enter, prototype.clone());

                    move |enter, call| match (&construct, call.new_target.clone()) {
                        (Some(construct), Some(new_target)) => B::instantiate(
                            enter.token(),
                            &prototype.for_target(enter, Some(&new_target))?,
                            construct(&CallContext::new(enter, &state), &Args::new(call))?,
                        ),
                        (_, new_target) => Err(B::throw(
                            enter.token(),
                            B::error(
                                enter.token(),
                                GuestErrorKind::TypeError,
                                &match new_target {
                                    Some(_) => "Illegal constructor".to_owned(),
                                    None => format!(
                                        "Class constructor {} cannot be invoked without 'new'",
                                        C::NAME,
                                    ),
                                },
                            )?,
                        )),
                    }
                }),
            )?;

            self.statics.install(enter, state, &constructor)?;

            RealisedClass::link(enter, constructor, prototype)
        })
    }
}

impl<B: Backend, C: Payload> Default for ClassDeclaration<B, C> {
    fn default() -> Self {
        Self::new()
    }
}

impl<B, C> Declaration<B> for ClassDeclaration<B, C>
where
    B: BackendClasses + BackendExceptions,
    C: Payload,
{
    fn realise<'js>(
        &self,
        enter: &Enter<'js, B>,
        state: &HostState,
        _name: &str,
    ) -> Result<Val<'js, B>, Error> {
        self.realised(enter, state)?
            .constructor(enter)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use crate::{
        backend::{
            classes::BackendClasses,
            exceptions::BackendExceptions,
            promises::BackendPromises,
            tests::Stub,
            values::WellKnown,
            Backend,
        },
        enter::Enter,
        errors::Error,
        guest::Guest,
        handle::{
            class::{
                Class,
                Instance,
                Payload,
            },
            value::Value,
        },
        host::{
            class::{
                ClassDeclaration,
                ClassPrototype,
                HostClass,
                RealisedClass,
                RealisedClasses,
            },
            declaration::Declaration,
            namespace::NamespaceDeclaration,
            property::PropertyDeclaration,
            value::ValueDeclaration,
        },
        marshal::ToGuest,
        runtime::Runtime,
    };

    struct Counter {
        count: f64,
    }

    impl Payload for Counter {
        const NAME: &'static str = "Counter";
    }

    impl<B> HostClass<B> for Counter
    where
        B: BackendClasses + BackendExceptions,
    {
        fn declaration() -> ClassDeclaration<B, Self> {
            ClassDeclaration::new()
                .constructor(|context, args| {
                    Ok(Counter {
                        count: args.next::<f64>(context.enter())?,
                    })
                })
                .method("increment", |context, args| {
                    let mut counter = args.this_mut::<Counter>(context.enter())?;

                    counter.count += 1.0;

                    Ok(counter.count)
                })
                .getter("count", |context, args| {
                    Ok(args.this_ref::<Counter>(context.enter())?.count)
                })
                .setter("count", |context, args| {
                    args.this_mut::<Counter>(context.enter())?.count =
                        args.next::<f64>(context.enter())?;

                    Ok(())
                })
                .constant("step", 1.0)
                .statics({
                    let mut namespace = NamespaceDeclaration::new();

                    namespace.value("zero", 0.0);

                    namespace
                })
        }
    }

    struct Labelled<B: Backend> {
        label: Value<B>,
    }

    impl<B: Backend> Payload for Labelled<B> {
        const NAME: &'static str = "Labelled";
    }

    impl<B> HostClass<B> for Labelled<B>
    where
        B: BackendClasses + BackendExceptions,
    {
        fn declaration() -> ClassDeclaration<B, Self> {
            ClassDeclaration::new()
                .constructor(|context, args| {
                    Ok(Labelled {
                        label: args.next::<Value<B>>(context.enter())?,
                    })
                })
                .getter("label", |context, args| {
                    Ok(args
                        .this_ref::<Labelled<B>>(context.enter())?
                        .label
                        .clone())
                })
        }
    }

    struct First;

    struct Second;

    #[test]
    fn a_class_is_realised_once_per_type() {
        let realisations = Cell::new(0);

        Guest::new(&Runtime::<Stub>::new(()).unwrap())
            .unwrap()
            .enter(|enter| {
                for _ in 0..2 {
                    RealisedClasses::of(enter.guest()).get_or_realise::<First, _>(|| {
                        realisations.set(realisations.get() + 1);

                        Ok(RealisedClass::new(enter, (), ClassPrototype::new(enter, ())))
                    })?;
                }

                Ok(())
            })
            .unwrap();

        assert_eq!(realisations.get(), 1);
    }

    #[test]
    fn each_type_is_realised_separately() {
        Guest::new(&Runtime::<Stub>::new(()).unwrap())
            .unwrap()
            .enter(|enter| {
                RealisedClasses::of(enter.guest()).get_or_realise::<First, _>(|| {
                    Ok(RealisedClass::new(enter, (), ClassPrototype::new(enter, ())))
                })?;

                assert!(RealisedClasses::of(enter.guest()).get::<Second>().is_none());

                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn realised_classes_are_kept_per_guest() {
        let runtime = Runtime::<Stub>::new(()).unwrap();

        Guest::new(&runtime)
            .unwrap()
            .enter(|enter| {
                RealisedClasses::of(enter.guest()).get_or_realise::<First, _>(|| {
                    Ok(RealisedClass::new(enter, (), ClassPrototype::new(enter, ())))
                })?;

                Ok(())
            })
            .unwrap();

        Guest::new(&runtime)
            .unwrap()
            .enter(|enter| {
                assert!(RealisedClasses::of(enter.guest()).get::<First>().is_none());

                Ok(())
            })
            .unwrap();
    }

    #[allow(dead_code)]
    fn capabilities<B>()
    where
        B: BackendClasses + BackendPromises + BackendExceptions,
    {
        fn declaration<B: BackendClasses + BackendExceptions, T: Declaration<B>>() {}
        fn to<B: BackendClasses, T: ToGuest<B>>() {}
        fn class<B>(
            enter: &Enter<'_, B>,
        ) -> Result<Class<B, Instance<B, Counter>>, Error>
        where
            B: BackendClasses + BackendExceptions,
        {
            Counter::class(enter)
        }

        let _ = ClassDeclaration::<B, Counter>::new()
            .method(WellKnown::Iterator, |_, _| Ok::<_, Error>(()))
            .method_async("wait", |_, _| {
                Ok::<_, Error>(async { Ok::<_, Error>(()) })
            })
            .value("tag", "counter")
            .property(
                "raw",
                PropertyDeclaration::constant(ValueDeclaration::new(1.0)),
            );

        declaration::<B, ClassDeclaration<B, Counter>>();
        declaration::<B, ClassDeclaration<B, Labelled<B>>>();
        to::<B, Counter>();
        to::<B, Labelled<B>>();
    }
}
