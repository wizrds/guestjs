use std::cell::Cell;

use crate::{
    backend::{
        classes::BackendClasses, exceptions::BackendExceptions, interrupt::BackendInterrupt,
        modules::BackendModules, promises::BackendPromises, values::WellKnown,
    },
    builder::{GuestBuilderExt, RuntimeBuilderExt},
    errors::Error,
    eval::{GuestEvalExt, GuestGlobalsExt},
    fixtures::guest_fixture,
    guest::Guest,
    handle::{
        HasHandle,
        class::{Class, Instance, Payload},
        function::Function,
        iter::AsyncIter,
        object::Object,
        promise::Promise,
        protocol::{CallableProtocol, ConstructorProtocol, ObjectProtocol, ValueProtocol},
    },
    host::{
        class::{ClassDeclaration, HostClass},
        module::ModuleDeclaration,
    },
    imports::GuestModulesExt,
    marshal::{ToGuest, collections::IterItem},
    runtime::Runtime,
};

struct Vector2 {
    x: f64,
    y: f64,
}

impl Vector2 {
    fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    fn x(&self) -> f64 {
        self.x
    }

    fn y(&self) -> f64 {
        self.y
    }

    fn set_x(&mut self, x: f64) {
        self.x = x;
    }

    fn length(&self) -> f64 {
        self.x.hypot(self.y)
    }
}

impl Payload for Vector2 {
    const NAME: &'static str = "Vector2";
}

impl<B> HostClass<B> for Vector2
where
    B: BackendClasses + BackendExceptions,
{
    fn declaration() -> ClassDeclaration<B, Self> {
        ClassDeclaration::new()
            .constructor(|context, args| {
                Ok(Self::new(
                    args.next::<f64>(context.enter())?,
                    args.next::<f64>(context.enter())?,
                ))
            })
            .method("length", |context, args| {
                Ok(args
                    .this_ref::<Self>(context.enter())?
                    .length())
            })
            .getter("x", |context, args| {
                Ok(args
                    .this_ref::<Self>(context.enter())?
                    .x())
            })
            .setter("x", |context, args| {
                args.this_mut::<Self>(context.enter())?
                    .set_x(args.next::<f64>(context.enter())?);

                Ok(())
            })
    }
}

struct Contract;

impl Payload for Contract {
    const NAME: &'static str = "Contract";
}

impl<B> HostClass<B> for Contract
where
    B: BackendClasses + BackendPromises + BackendExceptions,
{
    fn declaration() -> ClassDeclaration<B, Self> {
        ClassDeclaration::new()
            .constructor(|_, _| Ok(Self))
            .method_async("invoke", |context, args| {
                let this = args.this::<Object<B>>(context.enter())?;
                let city = args.next::<String>(context.enter())?;

                Ok(async move { this.call_method::<_, _, String>("execute", (city,)) })
            })
    }
}

struct Ledger {
    prefix: String,
}

impl Ledger {
    fn new(prefix: String) -> Self {
        Self { prefix }
    }

    fn prefix(&self) -> String {
        self.prefix.clone()
    }
}

impl Payload for Ledger {
    const NAME: &'static str = "Ledger";
}

impl<B> HostClass<B> for Ledger
where
    B: BackendClasses + BackendPromises + BackendExceptions,
{
    fn declaration() -> ClassDeclaration<B, Self> {
        ClassDeclaration::new()
            .constructor(|context, args| Ok(Self::new(args.next::<String>(context.enter())?)))
            .method("describe", |context, args| {
                let this = args.this::<Instance<B, Self>>(context.enter())?;

                Ok(format!(
                    "{}/{}",
                    this.borrow_with(Self::prefix)?,
                    this.call_method::<_, _, String>("label", ())?,
                ))
            })
            .method_async("describe_later", |context, args| {
                let this = args.this::<Object<B>>(context.enter())?;
                let prefix = args
                    .this_ref::<Self>(context.enter())?
                    .prefix();

                Ok(async move {
                    Ok::<_, Error>(format!(
                        "{}/{}",
                        prefix,
                        this.call_method::<_, _, String>("label", ())?,
                    ))
                })
            })
    }
}

struct Awaitable {
    value: i64,
}

impl Awaitable {
    fn new(value: i64) -> Self {
        Self { value }
    }

    fn value(&self) -> i64 {
        self.value
    }
}

impl Payload for Awaitable {
    const NAME: &'static str = "Awaitable";
}

impl<B> HostClass<B> for Awaitable
where
    B: BackendClasses + BackendExceptions,
{
    fn declaration() -> ClassDeclaration<B, Self> {
        ClassDeclaration::new()
            .constructor(|context, args| Ok(Self::new(args.next::<i64>(context.enter())?)))
            .method("then", |context, args| {
                let value = args
                    .this_ref::<Self>(context.enter())?
                    .value();

                args.next::<Function<B>>(context.enter())?
                    .call::<_, ()>((value,))
            })
    }
}

struct Sequence {
    value: Cell<i64>,
}

impl Sequence {
    fn new() -> Self {
        Self { value: Cell::new(0) }
    }

    fn next_result(&self) -> IterItem<i64> {
        let next = self.value.get() + 1;

        self.value.set(next);

        IterItem::new((next <= 2).then_some(next))
    }
}

impl Payload for Sequence {
    const NAME: &'static str = "Sequence";
}

impl<B> HostClass<B> for Sequence
where
    B: BackendClasses + BackendPromises + BackendExceptions,
{
    fn declaration() -> ClassDeclaration<B, Self> {
        ClassDeclaration::new()
            .constructor(|_, _| Ok(Self::new()))
            .method(WellKnown::AsyncIterator, |context, args| {
                args.this::<Object<B>>(context.enter())
            })
            .method_async("next", |context, args| {
                let result = args
                    .this_ref::<Self>(context.enter())?
                    .next_result();

                Ok(async move { Ok::<_, Error>(result) })
            })
    }
}

struct Session {
    count: i64,
}

impl Session {
    fn new() -> Self {
        Self { count: 0 }
    }

    fn count(&self) -> i64 {
        self.count
    }

    fn set_count(&mut self, count: i64) {
        self.count = count;
    }
}

impl Payload for Session {
    const NAME: &'static str = "Session";
}

impl<B> HostClass<B> for Session
where
    B: BackendClasses + BackendExceptions,
{
    fn declaration() -> ClassDeclaration<B, Self> {
        ClassDeclaration::new()
            .constructor(|_, _| Ok(Self::new()))
            .getter("count", |context, args| {
                Ok(args
                    .this_ref::<Self>(context.enter())?
                    .count())
            })
            .setter("count", |context, args| {
                args.this_mut::<Self>(context.enter())?
                    .set_count(args.next::<i64>(context.enter())?);

                Ok(())
            })
    }
}

struct HostConstructionOnly {
    value: i64,
}

impl HostConstructionOnly {
    fn new(value: i64) -> Self {
        Self { value }
    }

    fn value(&self) -> i64 {
        self.value
    }
}

impl Payload for HostConstructionOnly {
    const NAME: &'static str = "HostConstructionOnly";
}

impl<B> HostClass<B> for HostConstructionOnly
where
    B: BackendClasses + BackendExceptions,
{
    fn declaration() -> ClassDeclaration<B, Self> {
        ClassDeclaration::new().getter("value", |context, args| {
            Ok(args
                .this_ref::<Self>(context.enter())?
                .value())
        })
    }
}

struct ClassModules;

impl ClassModules {
    fn install<B>(guest: &Guest<B>, name: &str)
    where
        B: BackendModules + BackendPromises + BackendExceptions,
    {
        guest
            .globals()
            .unwrap()
            .set(name, guest.host_module(name).unwrap())
            .unwrap();
    }
}

guest_fixture! {
    pub fn host_borrows_dynamic_and_typed_payloads<B>()
    where B: [
        BackendModules,
        BackendInterrupt,
        BackendClasses,
        BackendPromises,
        BackendExceptions,
    ]
    using Runtime::<B>::builder().bind(
        ModuleDeclaration::new("geometry").class::<Vector2>(),
    );
    |guest| {
        ClassModules::install(&guest, "geometry");

        let dynamic = guest
            .eval::<Instance<B>>("new geometry.Vector2(3, 4)")
            .unwrap();
        let typed = dynamic.as_typed::<Vector2>().unwrap();

        assert_eq!(
            dynamic.borrow_as_with::<Vector2, _, _>(Vector2::x).unwrap(),
            3.0,
        );

        typed.borrow_with_mut(|vector| vector.set_x(1.0)).unwrap();

        assert_eq!(typed.borrow_with(Vector2::x).unwrap(), 1.0);
    }
}

guest_fixture! {
    pub fn ordinary_guest_class_uses_instance_handle<B>()
    where B: [
        BackendModules,
        BackendInterrupt,
    ]
    using Runtime::<B>::builder();
    |guest| {
        guest
            .exec(
                r#"
        class GuestVector {
            constructor(x) {
                this.x = x;
            }
        }

        globalThis.value = new GuestVector(3);
        "#,
            )
            .unwrap();

        assert_eq!(
            guest
                .globals()
                .unwrap()
                .get::<Instance<B>, _>("value")
                .unwrap()
                .get::<i64, _>("x")
                .unwrap(),
            3,
        );
    }
}

guest_fixture! {
    pub fn class_constructs_and_overrides_its_result_descriptor<B>()
    where B: [
        BackendModules,
        BackendInterrupt,
        BackendClasses,
        BackendPromises,
        BackendExceptions,
    ]
    using Runtime::<B>::builder().bind(
        ModuleDeclaration::new("geometry").class::<Vector2>(),
    );
    |guest| {
        ClassModules::install(&guest, "geometry");

        let class = guest.eval::<Class<B, Vector2>>("geometry.Vector2").unwrap();
        let retyped = class.with_result::<Instance<B>>();

        assert_eq!(
            class
                .construct((3.0, 4.0))
                .unwrap()
                .borrow_with(Vector2::length)
                .unwrap(),
            5.0,
        );
        assert_eq!(
            class
                .construct_as::<_, Instance<B>>((6.0, 8.0))
                .unwrap()
                .get::<f64, _>("x")
                .unwrap(),
            6.0,
        );
        assert_eq!(
            retyped
                .construct((5.0, 12.0))
                .unwrap()
                .borrow_as_with::<Vector2, _, _>(Vector2::y)
                .unwrap(),
            12.0,
        );
        assert!(class.handle().same(retyped.handle()));
    }
}

guest_fixture! {
    pub fn guest_subclass_works<B>()
    where B: [
        BackendModules,
        BackendInterrupt,
        BackendClasses,
        BackendPromises,
        BackendExceptions,
    ]
    using Runtime::<B>::builder().bind(
        ModuleDeclaration::new("geometry").class::<Vector2>(),
    );
    |guest| {
        ClassModules::install(&guest, "geometry");

        guest
            .exec(
                r#"
        class Tagged extends geometry.Vector2 {
            constructor(x, y) {
                super(x, y);
                this.tag = "tagged";
            }
        }

        globalThis.tagged = new Tagged(3, 4);
        "#,
            )
            .unwrap();

        let tagged = guest.eval::<Vector2>("tagged").unwrap();

        assert_eq!(tagged.borrow_with(Vector2::length).unwrap(), 5.0);
        assert_eq!(tagged.get::<String, _>("tag").unwrap(), "tagged");
    }
}

guest_fixture! {
    pub fn subclass_without_super_has_no_host_payload<B>()
    where B: [
        BackendModules,
        BackendInterrupt,
        BackendClasses,
        BackendPromises,
        BackendExceptions,
    ]
    using Runtime::<B>::builder().bind(
        ModuleDeclaration::new("geometry").class::<Vector2>(),
    );
    |guest| {
        ClassModules::install(&guest, "geometry");

        guest
            .exec(
                r#"
        class Empty extends geometry.Vector2 {
            constructor() {
                return {};
            }
        }

        class Missing extends geometry.Vector2 {
            constructor() {}
        }

        globalThis.empty = new Empty();
        "#,
            )
            .unwrap();

        assert!(guest.eval::<Object<B>>("empty").is_ok());
        assert!(guest
            .eval::<Vector2>("empty")
            .is_err_and(|error| error.is_conversion()));
        assert!(matches!(
            guest.eval::<Object<B>>("new Missing()"),
            Err(Error::Guest(exception)) if exception.matches("ReferenceError"),
        ));
    }
}

guest_fixture! {
    pub fn class_handle_reads_class_attributes<B>()
    where B: [
        BackendModules,
        BackendInterrupt,
    ]
    using Runtime::<B>::builder();
    |guest| {
        guest
            .exec(
                r#"
        class Impl {
            static description = "a description";
        }
        "#,
            )
            .unwrap();

        let class = guest.eval::<Class<B>>("Impl").unwrap();

        assert_eq!(
            class.get::<String, _>("description").unwrap(),
            "a description",
        );
        assert!(class.has("description").unwrap());
        assert!(!class.has("missing").unwrap());
        assert!(class.keys::<String>().unwrap().contains(&"description".to_owned()));
        assert_eq!(class.get::<String, _>("name").unwrap(), "Impl");
    }
}

guest_fixture! {
    pub fn functions_expose_javascript_runtime_metadata<B>()
    where B: [
        BackendModules,
        BackendInterrupt,
        BackendPromises,
        BackendExceptions,
    ]
    using Runtime::<B>::builder();
    |guest| {
        let function = guest
            .guest_module(
                "probe.js",
                "export function scale(value, factor) { return value * factor; }",
            )
            .unwrap()
            .function("scale")
            .unwrap();

        assert_eq!(function.get::<String, _>("name").unwrap(), "scale");
        assert_eq!(function.get::<usize, _>("length").unwrap(), 2);
    }
}

guest_fixture! {
    pub fn class_reports_its_prototype_chain<B>()
    where B: [
        BackendModules,
        BackendInterrupt,
    ]
    using Runtime::<B>::builder();
    |guest| {
        guest
            .exec(
                r#"
        class Base {}
        class Derived extends Base {}
        "#,
            )
            .unwrap();

        assert!(guest
            .enter(|enter| {
                let base = B::eval(enter.token(), "Base", "base.js")?;
                let derived = B::eval(enter.token(), "Derived", "derived.js")?;

                Ok(B::same(
                    enter.token(),
                    &B::prototype_of(enter.token(), &derived)?,
                    &base,
                ) && B::same(
                    enter.token(),
                    &B::prototype_of(
                        enter.token(),
                        &B::get(
                            enter.token(),
                            &derived,
                            &"prototype".to_guest(enter)?,
                        )?,
                    )?,
                    &B::get(
                        enter.token(),
                        &base,
                        &"prototype".to_guest(enter)?,
                    )?,
                ))
            })
            .unwrap());
    }
}

guest_fixture! {
    pub fn isinstance_and_issubclass_agree_across_backends<B>()
    where B: [
        BackendModules,
        BackendInterrupt,
        BackendClasses,
        BackendPromises,
        BackendExceptions,
    ]
    using Runtime::<B>::builder().bind(
        ModuleDeclaration::new("host_lib").class::<Contract>(),
    );
    |guest| {
        ClassModules::install(&guest, "host_lib");

        guest
            .exec(
                r#"
        class Impl extends host_lib.Contract {}
        class Plain {}
        globalThis.instance = new Impl();
        globalThis.other = new Plain();
        "#,
            )
            .unwrap();

        let contract = guest.eval::<Class<B>>("host_lib.Contract").unwrap();
        let implementation = guest.eval::<Class<B>>("Impl").unwrap();
        let plain = guest.eval::<Class<B>>("Plain").unwrap();
        let instance = guest.eval::<Object<B>>("instance").unwrap();
        let other = guest.eval::<Object<B>>("other").unwrap();

        assert!(instance.is_instance_of(&contract).unwrap());
        assert!(instance.is_instance_of(&implementation).unwrap());
        assert!(!other.is_instance_of(&contract).unwrap());
        assert!(implementation.is_subclass_of(&contract).unwrap());
        assert!(!plain.is_subclass_of(&contract).unwrap());
        assert!(contract.is_subclass_of(&contract).unwrap());
    }
}

guest_fixture! {
    pub async fn host_base_method_calls_guest_override<B>()
    where B: [
        BackendModules,
        BackendInterrupt,
        BackendClasses,
        BackendPromises,
        BackendExceptions,
    ]
    using Runtime::<B>::builder().bind(
        ModuleDeclaration::new("host_lib").class::<Contract>(),
    );
    |guest| {
        ClassModules::install(&guest, "host_lib");

        guest
            .exec(
                r#"
        class Impl extends host_lib.Contract {
            execute(city) {
                return "sunny in " + city;
            }
        }
        "#,
            )
            .unwrap();

        assert_eq!(
            guest
                .eval::<Instance<B>>("new Impl()")
                .unwrap()
                .call_method::<_, _, Promise<B, String>>("invoke", ("Vancouver",))
                .unwrap()
                .await
                .unwrap(),
            "sunny in Vancouver",
        );
    }
}

guest_fixture! {
    pub fn paired_method_reads_payload_and_calls_override<B>()
    where B: [
        BackendModules,
        BackendInterrupt,
        BackendClasses,
        BackendPromises,
        BackendExceptions,
    ]
    using Runtime::<B>::builder().bind(
        ModuleDeclaration::new("host_lib").class::<Ledger>(),
    );
    |guest| {
        ClassModules::install(&guest, "host_lib");

        guest
            .exec(
                r#"
        class Detailed extends host_lib.Ledger {
            label() {
                return "detailed";
            }
        }
        "#,
            )
            .unwrap();

        assert_eq!(
            guest
                .eval::<Instance<B>>("new Detailed('ledger')")
                .unwrap()
                .call_method::<_, _, String>("describe", ())
                .unwrap(),
            "ledger/detailed",
        );
    }
}

guest_fixture! {
    pub async fn paired_async_method_reads_payload_and_calls_override<B>()
    where B: [
        BackendModules,
        BackendInterrupt,
        BackendClasses,
        BackendPromises,
        BackendExceptions,
    ]
    using Runtime::<B>::builder().bind(
        ModuleDeclaration::new("host_lib").class::<Ledger>(),
    );
    |guest| {
        ClassModules::install(&guest, "host_lib");

        guest
            .exec(
                r#"
        class Detailed extends host_lib.Ledger {
            label() {
                return "detailed";
            }
        }
        "#,
            )
            .unwrap();

        assert_eq!(
            guest
                .eval::<Instance<B>>("new Detailed('ledger')")
                .unwrap()
                .call_method::<_, _, Promise<B, String>>("describe_later", ())
                .unwrap()
                .await
                .unwrap(),
            "ledger/detailed",
        );
    }
}

guest_fixture! {
    pub fn dynamic_handles_use_checked_callable_conversion<B>()
    where B: [
        BackendModules,
        BackendInterrupt,
        BackendPromises,
        BackendExceptions,
    ]
    using Runtime::<B>::builder().bind(ModuleDeclaration::new("host_lib"));
    |guest| {
        guest.exec("function twice(value) { return value * 2; }").unwrap();

        assert_eq!(
            guest
                .eval::<Object<B>>("twice")
                .unwrap()
                .cast::<Function<B>>()
                .unwrap()
                .call::<_, i64>((21i64,))
                .unwrap(),
            42,
        );
        assert!(guest
            .host_module("host_lib")
            .unwrap()
            .cast::<Function<B>>()
            .is_err_and(|error| error.is_conversion()));
    }
}

guest_fixture! {
    pub async fn awaiting_a_host_thenable_yields_its_value<B>()
    where B: [
        BackendModules,
        BackendInterrupt,
        BackendClasses,
        BackendPromises,
        BackendExceptions,
    ]
    using Runtime::<B>::builder().bind(
        ModuleDeclaration::new("host_lib").class::<Awaitable>(),
    );
    |guest| {
        ClassModules::install(&guest, "host_lib");

        assert_eq!(
            guest
                .eval::<Promise<B, i64>>(
                    "(async () => await new host_lib.Awaitable(7))()",
                )
                .unwrap()
                .await
                .unwrap(),
            7,
        );
    }
}

guest_fixture! {
    pub async fn host_async_iteration_ends_with_done<B>()
    where B: [
        BackendModules,
        BackendInterrupt,
        BackendClasses,
        BackendPromises,
        BackendExceptions,
    ]
    using Runtime::<B>::builder().bind(
        ModuleDeclaration::new("host_lib").class::<Sequence>(),
    );
    |guest| {
        ClassModules::install(&guest, "host_lib");

        let mut sequence = guest
            .eval::<AsyncIter<B, i64>>("new host_lib.Sequence()")
            .unwrap();

        assert_eq!(sequence.next().await.unwrap(), Some(1));
        assert_eq!(sequence.next().await.unwrap(), Some(2));
        assert_eq!(sequence.next().await.unwrap(), None);
    }
}

guest_fixture! {
    pub fn setter_mutates_then_getter_reads_it_back<B>()
    where B: [
        BackendModules,
        BackendInterrupt,
        BackendClasses,
        BackendPromises,
        BackendExceptions,
    ]
    using Runtime::<B>::builder().bind(
        ModuleDeclaration::new("host_lib").class::<Session>(),
    );
    |guest| {
        ClassModules::install(&guest, "host_lib");

        guest
            .exec(
                r#"
        globalThis.session = new host_lib.Session();
        session.count = 3;
        "#,
            )
            .unwrap();

        assert_eq!(guest.eval::<i64>("session.count").unwrap(), 3);
    }
}

guest_fixture! {
    pub fn host_instantiates_host_class_with_payload<B>()
    where B: [
        BackendModules,
        BackendInterrupt,
        BackendClasses,
        BackendPromises,
        BackendExceptions,
    ]
    using Runtime::<B>::builder().bind(
        ModuleDeclaration::new("host_lib").class::<HostConstructionOnly>(),
    );
    |guest| {
        ClassModules::install(&guest, "host_lib");

        assert_eq!(
            guest
                .enter(|enter| {
                    HostConstructionOnly::class(enter)?
                        .instantiate(HostConstructionOnly::new(42))
                })
                .unwrap()
                .get::<i64, _>("value")
                .unwrap(),
            42,
        );
        assert!(matches!(
            guest.eval::<Instance<B>>("new host_lib.HostConstructionOnly(5)"),
            Err(Error::Guest(exception))
                if exception.matches("TypeError")
                    && exception.message().contains("Illegal constructor"),
        ));
    }
}

pub fn host_classes_are_realised_per_guest<B>()
where
    B: BackendModules + BackendInterrupt + BackendClasses + BackendPromises + BackendExceptions,
{
    let runtime = Runtime::<B>::builder()
        .bind(ModuleDeclaration::new("host_lib").class::<HostConstructionOnly>())
        .build()
        .unwrap();
    let first = runtime.guest().build().unwrap();
    let second = runtime.guest().build().unwrap();
    let first_class = first
        .host_module("host_lib")
        .unwrap()
        .class("HostConstructionOnly")
        .unwrap();
    let second_class = second
        .host_module("host_lib")
        .unwrap()
        .class("HostConstructionOnly")
        .unwrap();

    assert!(
        first_class.handle().same(
            first
                .host_module("host_lib")
                .unwrap()
                .class("HostConstructionOnly")
                .unwrap()
                .handle(),
        )
    );
    assert!(
        !first_class
            .handle()
            .same(second_class.handle())
    );

    first_class.set("marker", 1i64).unwrap();

    assert!(!second_class.has("marker").unwrap());
    assert!(
        second
            .enter(|enter| first_class.to_guest(enter).map(drop))
            .is_err_and(|error| error.is_foreign_context())
    );
}

#[macro_export]
macro_rules! __guestjs_backend_classes_tests {
    ($backend:ty) => {
        #[test]
        fn host_borrows_dynamic_and_typed_payloads() {
            $crate::fixtures::classes::host_borrows_dynamic_and_typed_payloads::<$backend>();
        }

        #[test]
        fn ordinary_guest_class_uses_instance_handle() {
            $crate::fixtures::classes::ordinary_guest_class_uses_instance_handle::<$backend>();
        }

        #[test]
        fn class_constructs_and_overrides_its_result_descriptor() {
            $crate::fixtures::classes::class_constructs_and_overrides_its_result_descriptor::<
                $backend,
            >();
        }

        #[test]
        fn guest_subclass_works() {
            $crate::fixtures::classes::guest_subclass_works::<$backend>();
        }

        #[test]
        fn subclass_without_super_has_no_host_payload() {
            $crate::fixtures::classes::subclass_without_super_has_no_host_payload::<$backend>();
        }

        #[test]
        fn class_handle_reads_class_attributes() {
            $crate::fixtures::classes::class_handle_reads_class_attributes::<$backend>();
        }

        #[test]
        fn functions_expose_javascript_runtime_metadata() {
            $crate::fixtures::classes::functions_expose_javascript_runtime_metadata::<$backend>();
        }

        #[test]
        fn class_reports_its_prototype_chain() {
            $crate::fixtures::classes::class_reports_its_prototype_chain::<$backend>();
        }

        #[test]
        fn isinstance_and_issubclass_agree_across_backends() {
            $crate::fixtures::classes::isinstance_and_issubclass_agree_across_backends::<$backend>(
            );
        }

        #[tokio::test]
        async fn host_base_method_calls_guest_override() {
            $crate::fixtures::classes::host_base_method_calls_guest_override::<$backend>().await;
        }

        #[test]
        fn paired_method_reads_payload_and_calls_override() {
            $crate::fixtures::classes::paired_method_reads_payload_and_calls_override::<$backend>();
        }

        #[tokio::test]
        async fn paired_async_method_reads_payload_and_calls_override() {
            $crate::fixtures::classes::paired_async_method_reads_payload_and_calls_override::<
                $backend,
            >()
            .await;
        }

        #[test]
        fn dynamic_handles_use_checked_callable_conversion() {
            $crate::fixtures::classes::dynamic_handles_use_checked_callable_conversion::<$backend>(
            );
        }

        #[tokio::test]
        async fn awaiting_a_host_thenable_yields_its_value() {
            $crate::fixtures::classes::awaiting_a_host_thenable_yields_its_value::<$backend>()
                .await;
        }

        #[tokio::test]
        async fn host_async_iteration_ends_with_done() {
            $crate::fixtures::classes::host_async_iteration_ends_with_done::<$backend>().await;
        }

        #[test]
        fn setter_mutates_then_getter_reads_it_back() {
            $crate::fixtures::classes::setter_mutates_then_getter_reads_it_back::<$backend>();
        }

        #[test]
        fn host_instantiates_host_class_with_payload() {
            $crate::fixtures::classes::host_instantiates_host_class_with_payload::<$backend>();
        }

        #[test]
        fn host_classes_are_realised_per_guest() {
            $crate::fixtures::classes::host_classes_are_realised_per_guest::<$backend>();
        }
    };
}

pub use crate::__guestjs_backend_classes_tests as tests;
