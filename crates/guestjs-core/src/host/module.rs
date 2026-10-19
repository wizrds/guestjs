use std::{
    future::Future,
    rc::Rc,
};

use crate::{
    backend::{
        callables::BackendCallables,
        classes::BackendClasses,
        exceptions::BackendExceptions,
        promises::BackendPromises,
        Backend,
        Val,
    },
    enter::Enter,
    errors::Error,
    host::{
        class::HostClass,
        declaration::Declaration,
        exception::{
            ExceptionDeclaration,
            HostException,
        },
        function::{
            CallContext,
            FunctionDeclaration,
        },
        namespace::NamespaceDeclaration,
        state::HostState,
        value::ValueDeclaration,
    },
    marshal::{
        args::Args,
        ToGuest,
    },
};

pub struct ModuleDeclaration<B: Backend> {
    name: String,
    members: Vec<(String, Rc<dyn Declaration<B>>)>,
    state: Box<dyn for<'js> Fn(&Enter<'js, B>) -> Result<HostState, Error>>,
}

impl<B: Backend> ModuleDeclaration<B> {
    pub fn new<N>(name: N) -> Self
    where
        N: Into<String>,
    {
        Self {
            name: name.into(),
            members: Vec::new(),
            state: Box::new(|_| Ok(HostState::empty())),
        }
    }

    pub fn member<N, D>(mut self, name: N, declaration: D) -> Self
    where
        N: Into<String>,
        D: Declaration<B> + 'static,
    {
        self.members.push((name.into(), Rc::new(declaration)));

        self
    }

    pub fn value<N, T>(self, name: N, value: T) -> Self
    where
        N: Into<String>,
        T: ToGuest<B> + Clone + 'static,
    {
        self.member(name, ValueDeclaration::new(value))
    }

    pub fn state<S, F>(mut self, state: F) -> Self
    where
        S: 'static,
        F: for<'js> Fn(&Enter<'js, B>) -> Result<S, Error> + 'static,
    {
        self.state = Box::new(move |enter| state(enter).map(HostState::new));

        self
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn exports<'js>(
        &self,
        enter: &Enter<'js, B>,
    ) -> Result<Vec<(String, Val<'js, B>)>, Error> {
        let state = (self.state)(enter)?;

        self.members
            .iter()
            .map(|(name, declaration)| {
                Ok((name.clone(), declaration.realise(enter, &state, name)?))
            })
            .collect()
    }
}

impl<B: BackendCallables> ModuleDeclaration<B> {
    pub fn function<N, F, R>(self, name: N, function: F) -> Self
    where
        N: Into<String>,
        F: for<'e, 'js> Fn(
                &CallContext<'e, 'js, B>,
                &Args<'js, B>,
            ) -> Result<R, Error>
            + 'static,
        R: ToGuest<B>,
    {
        self.member(name, FunctionDeclaration::new(function))
    }

    pub fn namespace<N>(self, name: N, namespace: NamespaceDeclaration<B>) -> Self
    where
        N: Into<String>,
    {
        self.member(name, namespace)
    }
}

impl<B> ModuleDeclaration<B>
where
    B: BackendCallables + BackendPromises + BackendExceptions,
{
    pub fn function_async<N, F, Fut, R>(self, name: N, function: F) -> Self
    where
        N: Into<String>,
        F: for<'e, 'js> Fn(
                &CallContext<'e, 'js, B>,
                &Args<'js, B>,
            ) -> Result<Fut, Error>
            + 'static,
        Fut: Future<Output = Result<R, Error>> + 'static,
        R: ToGuest<B> + 'static,
    {
        self.member(name, FunctionDeclaration::new_async(function))
    }
}

impl<B> ModuleDeclaration<B>
where
    B: BackendClasses + BackendExceptions,
{
    pub fn class<C>(self) -> Self
    where
        C: HostClass<B>,
    {
        self.member(C::NAME, C::declaration())
    }

    pub fn exception<E>(self) -> Self
    where
        E: HostException,
    {
        self.member(E::NAME, ExceptionDeclaration::<E>::new())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        cell::Cell,
        rc::Rc,
    };

    use crate::{
        backend::{
            classes::BackendClasses,
            exceptions::BackendExceptions,
            promises::BackendPromises,
            tests::Stub,
            Val,
        },
        enter::Enter,
        errors::Error,
        guest::Guest,
        handle::class::Payload,
        host::{
            class::{
                ClassDeclaration,
                HostClass,
            },
            declaration::Declaration,
            exception::HostException,
            module::ModuleDeclaration,
            namespace::NamespaceDeclaration,
            state::HostState,
        },
        runtime::Runtime,
    };

    #[derive(Default)]
    struct Seen {
        state: Rc<Cell<Option<u8>>>,
    }

    impl Declaration<Stub> for Seen {
        fn realise<'js>(
            &self,
            _enter: &Enter<'js, Stub>,
            state: &HostState,
            _name: &str,
        ) -> Result<Val<'js, Stub>, Error> {
            self.state.set(state.get::<u8>().map(|state| *state));

            Ok(())
        }
    }

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

    #[test]
    fn exports_follow_declaration_order() {
        Guest::new(&Runtime::<Stub>::new(()).unwrap())
            .unwrap()
            .enter(|enter| {
                assert_eq!(
                    ModuleDeclaration::new("pair")
                        .member("b", Seen::default())
                        .member("a", Seen::default())
                        .exports(enter)?
                        .into_iter()
                        .map(|(name, _)| name)
                        .collect::<Vec<_>>(),
                    ["b", "a"],
                );

                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn members_receive_the_module_state() {
        let seen = Rc::new(Cell::new(None));

        Guest::new(&Runtime::<Stub>::new(()).unwrap())
            .unwrap()
            .enter(|enter| {
                ModuleDeclaration::new("stateful")
                    .state(|_| Ok(7_u8))
                    .member(
                        "a",
                        Seen {
                            state: seen.clone(),
                        },
                    )
                    .exports(enter)
            })
            .unwrap();

        assert_eq!(seen.get(), Some(7));
    }

    #[test]
    fn a_module_without_state_gives_empty_state() {
        let seen = Rc::new(Cell::new(Some(0)));

        Guest::new(&Runtime::<Stub>::new(()).unwrap())
            .unwrap()
            .enter(|enter| {
                ModuleDeclaration::new("stateless")
                    .member(
                        "a",
                        Seen {
                            state: seen.clone(),
                        },
                    )
                    .exports(enter)
            })
            .unwrap();

        assert_eq!(seen.get(), None);
    }

    #[test]
    fn state_is_built_for_each_realisation() {
        let builds = Rc::new(Cell::new(0_u8));

        Guest::new(&Runtime::<Stub>::new(()).unwrap())
            .unwrap()
            .enter(|enter| {
                let module = ModuleDeclaration::<Stub>::new("counted").state({
                    let builds = builds.clone();

                    move |_| {
                        builds.set(builds.get() + 1);

                        Ok(builds.get())
                    }
                });

                module.exports(enter)?;
                module.exports(enter)
            })
            .unwrap();

        assert_eq!(builds.get(), 2);
    }

    #[allow(dead_code)]
    fn capabilities<B>()
    where
        B: BackendClasses + BackendPromises + BackendExceptions,
    {
        let _ = ModuleDeclaration::<B>::new("geometry")
            .value("version", "1.0")
            .value("default", 1.0)
            .function("add", |_, _| Ok::<_, Error>(1.0))
            .function_async("fetch", |_, _| {
                Ok::<_, Error>(async { Ok::<_, Error>(1.0) })
            })
            .class::<Counter>()
            .exception::<NetworkError>()
            .namespace("units", NamespaceDeclaration::new().constant("METRE", 1.0));
    }
}
