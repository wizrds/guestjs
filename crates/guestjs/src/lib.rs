//! A host-to-guest module execution library for JavaScript and TypeScript.
//!
//! `guestjs` lets Rust applications create isolated [`Guest`](crate::runtime::Guest) contexts,
//! load guest modules, and use their exported values through Rust handles. A
//! [`Runtime`](crate::runtime::Runtime) owns the JavaScript engine and shared module registry.
//! [`RuntimeBuilder::bind`](crate::runtime::RuntimeBuilder::bind) and
//! [`RuntimeBuilder::bind_native`](crate::runtime::RuntimeBuilder::bind_native) grant modules to
//! every guest created by a runtime. The corresponding methods on
//! [`GuestBuilder`](crate::runtime::GuestBuilder) grant modules only to the guest being built.
//! Runtime bindings are applied before guest bindings, and the last registration for an import
//! specifier wins.
//!
//! Owned handles such as [`Module`](crate::handle::Module),
//! [`Function`](crate::handle::Function), [`Promise`](crate::handle::Promise), and
//! [`Awaitable`](crate::handle::Awaitable) retain their guest context and enter it for each
//! asynchronous operation.
//! [`Guest::scope`](crate::runtime::Guest::scope) enters the context once and supplies a
//! [`Scope`](crate::runtime::Scope). Module loading and descendant operations inside that callback
//! return bound handles tied to the live scope. Functions, objects, classes, instances, and
//! promise-like values remain bound automatically as operations return further guest values.
//!
//! JavaScript source can be loaded through
//! [`Guest::guest_module`](crate::runtime::Guest::guest_module) for an owned module or
//! [`Scope::guest_module`](crate::runtime::Scope::guest_module) for a bound module. Registered host
//! exports can likewise be opened through
//! [`Guest::host_module`](crate::runtime::Guest::host_module) or
//! [`Scope::host_module`](crate::runtime::Scope::host_module). When the `typescript` feature is
//! enabled, the configured transpiler receives the module name and source before guest-module
//! evaluation.
//!
//! # Guest buffers and carried values
//!
//! A host function fills a buffer the guest allocated by taking a
//! [`Uint8Array`](crate::handle::Uint8Array) or [`ArrayBuffer`](crate::handle::ArrayBuffer) argument
//! and writing through [`std::io::Write`](std::io::Write), which writes into the guest's own storage
//! rather than a copy. [`std::io::Read`](std::io::Read) and [`std::io::Seek`](std::io::Seek) are
//! available on the same handles. Any typed array additionally exposes
//! [`get`](crate::handle::BoundTypedArray::get) and [`set`](crate::handle::BoundTypedArray::set)
//! element access for its own element type.
//!
//! ```ignore
//! use guestjs::prelude::*;
//!
//! exports.function("fillSync", |scope, args| {
//!     args.get::<Uint8Array>(scope, 0)?
//!         .write_all(&[1, 2, 3, 4])?;
//!
//!     Ok(())
//! });
//! ```
//!
//! An asynchronous host callable carries a guest value across its own await by taking it as an owned
//! [`Value`](crate::handle::Value), which holds no scope, and binding it again after the await
//! inside a [`Scoped`](crate::handle::Scoped) return. The guest receives the object it passed in,
//! not a copy.
//!
//! ```ignore
//! use guestjs::prelude::*;
//!
//! exports.async_function("fillAsync", |scope, args| {
//!     let value = args.get_owned::<Value>(scope, 0)?;
//!
//!     Ok(async move {
//!         tokio::task::yield_now().await;
//!
//!         Ok(Scoped::new(move |scope: &Scope| {
//!             value
//!                 .bind::<Uint8Array>(scope)?
//!                 .write_all(&[1, 2, 3, 4])?;
//!
//!             Ok(value)
//!         }))
//!     })
//! });
//! ```
//!
//! The buffer handles have no owned byte access. A buffer view points into engine memory and is only
//! valid inside a scope, so byte reads and writes live on the bound forms alone. The owned handles
//! carry their guest context and expose the remaining surface, such as
//! [`TypedArray::len`](crate::handle::TypedArray::len) and
//! [`Array::get`](crate::handle::Array::get), by entering that context for each call.
//!
//! # Execution control
//!
//! Execution controls configured through
//! [`RuntimeBuilder`](crate::runtime::RuntimeBuilder) apply to every guest created by the runtime:
//!
//! ```ignore
//! use std::time::Duration;
//!
//! use guestjs::prelude::*;
//!
//! let cancellation = Cancellation::new();
//! let runtime = Runtime::builder()
//!     .memory_limit(64 * 1024 * 1024)
//!     .max_stack_size(512 * 1024)
//!     .gc_threshold(8 * 1024 * 1024)
//!     .execution_timeout(Duration::from_millis(50))
//!     .cancellation(cancellation.clone())
//!     .gc_after(128)
//!     .build()
//!     .await?;
//! let guest = runtime
//!     .guest()
//!     .build()
//!     .await?;
//!
//! assert!(matches!(
//!     guest
//!         .eval::<()>("while (true) {}")
//!         .await,
//!     Err(Error::Timeout),
//! ));
//!
//! cancellation.cancel();
//!
//! assert!(matches!(
//!     guest
//!         .eval::<i32>("1 + 1")
//!         .await,
//!     Err(Error::Cancelled),
//! ));
//!
//! runtime.run_gc().await;
//! ```
//!
//! [`RuntimeBuilder::memory_limit`](crate::runtime::RuntimeBuilder::memory_limit) limits engine
//! allocation, [`RuntimeBuilder::max_stack_size`](crate::runtime::RuntimeBuilder::max_stack_size)
//! limits the engine call stack, and
//! [`RuntimeBuilder::gc_threshold`](crate::runtime::RuntimeBuilder::gc_threshold) sets the
//! allocation threshold that triggers engine garbage collection.
//! [`RuntimeBuilder::gc_after`](crate::runtime::RuntimeBuilder::gc_after) runs garbage collection
//! after the configured number of guest executions.
//! [`Runtime::run_gc`](crate::runtime::Runtime::run_gc) requests a collection immediately.
//!
//! [`RuntimeBuilder::execution_timeout`](crate::runtime::RuntimeBuilder::execution_timeout) bounds
//! the time QuickJS spends executing each guest operation.
//! [`RuntimeBuilder::cancellation`](crate::runtime::RuntimeBuilder::cancellation) accepts any
//! [`CancelSignal`](crate::execution::CancelSignal).
//! [`Cancellation`](crate::execution::Cancellation) is a clonable, one-way signal that can stop an
//! active operation or reject a later operation. The `tokio` feature implements
//! [`CancelSignal`](crate::execution::CancelSignal) for Tokio cancellation tokens.
//!
//! [`RuntimeBuilder::interrupt_handler`](crate::runtime::RuntimeBuilder::interrupt_handler)
//! installs a custom synchronous interrupt condition. Returning `true` stops the current
//! execution with [`Error::Interrupted`](crate::errors::Error::Interrupted). Policy-driven
//! interrupts are reported as [`Error::Timeout`](crate::errors::Error::Timeout) or
//! [`Error::Cancelled`](crate::errors::Error::Cancelled).
//!
//! # Plain Rust data
//!
//! Deriving [`ToGuest`](crate::marshal::ToGuest) uses a type's
//! [`Serialize`](serde::Serialize) implementation, while deriving
//! [`FromGuest`](crate::marshal::FromGuest) uses its serde deserialization implementation. Either
//! direction can be derived independently. Serde attributes define the complete JavaScript
//! representation, including field names. An optional field with the value `None` becomes
//! JavaScript `null`:
//!
//! ```ignore
//! use guestjs::prelude::*;
//!
//! #[derive(
//!     Debug,
//!     PartialEq,
//!     serde::Serialize,
//!     serde::Deserialize,
//!     guestjs::ToGuest,
//!     guestjs::FromGuest,
//! )]
//! struct Request {
//!     #[serde(rename = "userId")]
//!     user_id: u64,
//!     note: Option<String>,
//! }
//!
//! assert_eq!(
//!     Runtime::builder()
//!         .build()
//!         .await?
//!         .guest()
//!         .build()
//!         .await?
//!         .guest_module(
//!             "request.js",
//!             r#"
//! export function normalize(request) {
//!     return {
//!         userId: request.userId,
//!         note: request.note,
//!     };
//! }
//! "#,
//!         )
//!         .await?
//!         .function("normalize")
//!         .await?
//!         .call::<_, Request>(
//!             (
//!                 Request {
//!                     user_id: 42,
//!                     note: None,
//!                 },
//!             ),
//!         )
//!         .await?,
//!     Request {
//!         user_id: 42,
//!         note: None,
//!     },
//! );
//! ```
//!
//! A [`HostClass`](crate::host::class::HostClass) already represents a guest object with identity
//! and borrowing behavior. Use a separate serde data type when the same state also needs plain-data
//! conversion. [`Nullish<T>`](crate::marshal::Nullish) preserves `undefined`, `null`, and present
//! values when converted directly, but it is not a supported field representation inside a
//! serde-derived aggregate.
//!
//! # Running guest code
//!
//! Owned operations are useful when values must be retained independently. A
//! [`Promise<T>`](crate::handle::Promise) requires a JavaScript promise.
//! [`Awaitable<T>`](crate::handle::Awaitable) accepts either a direct `T` or a promise resolving to
//! `T`. Both are awaited like Rust futures and produce the owned form of `T`.
//! [`guest_module!`](crate::guest_module) defines typed owned and bound access to an already-loaded
//! module. It does not load source, select a guest, or grant module authority.
//! Function and value declarations name their successful guest value; generated methods return
//! [`Result`](std::result::Result) with [`Error`](crate::errors::Error) for lookup, invocation, and
//! conversion failures:
//!
//! ```ignore
//! use guestjs::prelude::*;
//!
//! guestjs::guest_module! {
//!     pub module Math {
//!         fn add(
//!             left: i32,
//!             right: i32,
//!         ) -> i32;
//!
//!         fn double(
//!             value: i32,
//!         ) -> Promise<i32>;
//!
//!         #[guestjs(name = "makeAdder")]
//!         fn make_adder(
//!             amount: i32,
//!         ) -> Function;
//!
//!         fn apply(
//!             operation: Function,
//!             value: i32,
//!         ) -> i32;
//!
//!         fn multiplier(
//!             factor: i32,
//!         ) -> Promise<Function>;
//!
//!         fn status() -> Awaitable<String>;
//!
//!         value answer: i32;
//!         value settings: Object;
//!
//!         #[guestjs(name = "Counter")]
//!         value counter: Class;
//!
//!         #[guestjs(name = "pendingOperation")]
//!         value pending_operation: Promise<Function>;
//!     }
//! }
//!
//! let runtime = Runtime::builder()
//!     .memory_limit(16 * 1024 * 1024)
//!     .build()
//!     .await?;
//! let guest = runtime
//!     .guest()
//!     .build()
//!     .await?;
//!
//! assert_eq!(guest.eval::<i32>("6 * 7").await?, 42);
//!
//! let math = Math::from(
//!     guest
//!         .guest_module(
//!             "math.js",
//!             r#"
//! export function add(left, right) {
//!     return left + right;
//! }
//!
//! export async function double(value) {
//!     return value * 2;
//! }
//!
//! export function makeAdder(amount) {
//!     return value => value + amount;
//! }
//!
//! export function apply(operation, value) {
//!     return operation(value);
//! }
//!
//! export async function multiplier(factor) {
//!     return value => value * factor;
//! }
//!
//! export function status() {
//!     return "complete";
//! }
//!
//! export const answer = 42;
//! export const settings = {
//!     unit: "px",
//! };
//!
//! export class Counter {
//!     constructor(value) {
//!         this.value = value;
//!     }
//!
//!     increment() {
//!         return ++this.value;
//!     }
//! }
//!
//! export const pendingOperation = Promise.resolve(value => value * 2);
//! "#,
//!         )
//!         .await?,
//! );
//!
//! assert_eq!(
//!     math.add(2, 3).await?,
//!     5,
//! );
//! assert_eq!(
//!     math.double(21).await?.await?,
//!     42,
//! );
//! assert_eq!(math.answer().await?, 42);
//! assert_eq!(
//!     math.status().await?.await?,
//!     "complete",
//! );
//! assert_eq!(math.settings().await?.get::<String>("unit").await?, "px");
//! assert_eq!(
//!     math.counter()
//!         .await?
//!         .construct((1,))
//!         .await?
//!         .call::<_, i32>("increment", ())
//!         .await?,
//!     2,
//! );
//!
//! assert_eq!(
//!     guest
//!         .scope(async move |scope| {
//!             let math = math.bind(&scope)?;
//!
//!             assert_eq!(
//!                 math.apply(
//!                     math.make_adder(1)?,
//!                     41,
//!                 )?,
//!                 42,
//!             );
//!             assert_eq!(
//!                 math.multiplier(2)?
//!                     .await?
//!                     .call::<_, i32>((21,))?,
//!                 42,
//!             );
//!             assert_eq!(
//!                 math.pending_operation()?
//!                     .await?
//!                     .call::<_, i32>((21,))?,
//!                 42,
//!             );
//!
//!             math.make_adder(10)?
//!                 .into_owned()
//!         })
//!         .await?
//!         .call::<_, i32>((42,))
//!         .await?,
//!     52,
//! );
//! ```
//!
//! [`GuestType`](crate::marshal::GuestType) projects each declared input descriptor into its owned
//! or scope-bound argument type. Function declarations accept up to four parameters, matching the
//! current tuple conversion contract. Within a live scope, requesting the semantic descriptor
//! [`Function`](crate::handle::Function) returns a bound function, and requesting
//! [`Promise<Function>`](crate::handle::Promise) returns a bound promise whose resolved function is
//! also bound, while [`Awaitable<Function>`](crate::handle::Awaitable) accepts either a direct bound
//! function or a promise resolving to one. Bound handles implement the scoped argument conversions,
//! so a result can be passed directly into a later bound call. A value that must outlive the
//! callback is explicitly promoted with
//! [`BoundFunction::into_owned`](crate::handle::BoundFunction::into_owned). Each generated function
//! or value method performs a fresh export lookup; values are not cached by the facade.
//!
//! # Host classes and modules
//!
//! A [`HostClass`](crate::host::class::HostClass) stores Rust state in a guest object,
//! [`Args`](crate::host::args::Args) converts constructor and callable arguments, and
//! [`ClassSpec`](crate::host::class::ClassSpec) defines guest-visible members.
//! [`host_class`](crate::host_class) generates a [`HostClass`](crate::host::class::HostClass) from
//! an inherent implementation. Constructors, methods, accessors, well-known symbols, iteration,
//! static members, and owned asynchronous methods are exposed explicitly. A shared receiver
//! defines a shared method, while an exclusive receiver defines an exclusive method. Callable
//! errors may be any type that converts into [`Error`](crate::errors::Error). The guest class name
//! defaults to the Rust type name and can be overridden with `name`.
//!
//! Ordinary parameters use their [`FromGuestBound`](crate::marshal::FromGuestBound) descriptor.
//! [`Option<T>`](std::option::Option) treats an omitted, undefined, or null argument as `None`,
//! while [`Nullish<T>`](crate::marshal::Nullish) preserves undefined and null. Parameter helpers
//! inject a [`Scope`](crate::runtime::Scope), borrow another host-class instance, select a semantic
//! descriptor, or collect trailing arguments:
//!
//! ```text
//! #[guestjs(scope)] scope: &Scope<'_>
//! #[guestjs(borrow)] point: &Point
//! #[guestjs(borrow_mut)] point: &mut Point
//! #[guestjs(as = Function)] callback: BoundFunction<'_>
//! #[guestjs(rest)] values: Vec<f64>
//! ```
//!
//! `rename_all` accepts serde's `lowercase`, `UPPERCASE`, `PascalCase`, `camelCase`, `snake_case`,
//! `SCREAMING_SNAKE_CASE`, `kebab-case`, and `SCREAMING-KEBAB-CASE` conventions. An explicit member
//! `name` takes precedence. Getter and setter methods with the same final name form one accessor.
//! `symbol` accepts `iterator`, `asyncIterator`, `toPrimitive`, and `hasInstance`, while `iterable`
//! defines `Symbol.iterator` through the existing class iterator support. Associated constants,
//! static methods, and one statics hook define members on the class constructor.
//!
//! An `async_method` is a synchronous Rust method that returns an owned `'static` future. It reads
//! or mutates class state before returning the future, so no class borrow crosses an await point.
//! A borrowing Rust `async fn` is therefore not supported.
//!
//! A [`HostModule`](crate::host::module::HostModule) defines values that guest modules can import.
//! One module converts directly into a [`HostLibrary`](crate::host::library::HostLibrary), while
//! [`HostLibrary::with`](crate::host::library::HostLibrary::with) collects heterogeneous modules in
//! registration order and
//! [`HostLibrary::initialize`](crate::host::library::HostLibrary::initialize) adds setup that runs
//! once for each guest receiving the library. Binding through
//! [`RuntimeBuilder`](crate::runtime::RuntimeBuilder) grants the library to every guest; binding
//! through [`GuestBuilder`](crate::runtime::GuestBuilder) grants it only to that guest.
//! [`host_module`](crate::host_module) generates a
//! [`HostModule`](crate::host::module::HostModule) from an inherent implementation. Its `classes`
//! list preserves registration order, and associated functions annotated with `function` become
//! typed synchronous or asynchronous exports. Associated constants annotated with `default` or
//! `constant` define values. An `object` hook receives a
//! [`Namespace`](crate::host::namespace::Namespace) for defining nested functions, values,
//! properties, accessors, objects, and classes. One `build` hook receives the complete
//! [`Exports`](crate::host::module::Exports) and can define conditional or stateful exports.
//! One `init` hook receives a [`Scope`](crate::runtime::Scope) and runs once for each guest that
//! receives the host module.
//! Exported functions cannot have a receiver. Their errors may be any type that converts into
//! [`Error`](crate::errors::Error), and asynchronous parameters must own everything retained by
//! the returned future. Root accessors and live writable root bindings are unsupported; define
//! them inside an object hook instead.
//!
//! [`NativeModule`](crate::native::NativeModule) is the low-level escape hatch for adapting an
//! rquickjs module definition. One or more native modules and context initializers form a
//! [`NativeLibrary`](crate::native::NativeLibrary), which is registered with
//! [`RuntimeBuilder::bind_native`](crate::runtime::RuntimeBuilder::bind_native) or
//! [`GuestBuilder::bind_native`](crate::runtime::GuestBuilder::bind_native).
//!
//! The `llrt-buffer`, `llrt-console`, `llrt-fetch`, `llrt-fs`, `llrt-os`, `llrt-process-env`,
//! `llrt-timers`, and `llrt-url` features expose their corresponding
//! [`Llrt`](crate::llrt::Llrt) builder methods:
//!
//! ```ignore
//! use guestjs::{llrt::Llrt, prelude::*};
//!
//! let runtime = Runtime::builder()
//!     .bind_native(
//!         Llrt::builder()
//!             .buffer()
//!             .console()
//!             .timers()
//!             .url()
//!             .build(),
//!     )
//!     .build()
//!     .await?;
//!
//! let guest = runtime
//!     .guest()
//!     .bind_native(
//!         Llrt::builder()
//!             .fs()
//!             .fetch()
//!             .os()
//!             .process_env()
//!             .build(),
//!     )
//!     .build()
//!     .await?;
//! ```
//!
//! > `process_env` exposes only a host environment snapshot at `globalThis.process.env`. It does not
//! > provide the complete Node.js or LLRT process API. This adapter does not provide package or
//! > filesystem source resolution or complete Node compatibility.
//!
//! A complete host class and host module can be defined together:
//!
//! ```ignore
//! use std::future::Future;
//!
//! use guestjs::prelude::*;
//!
//! #[derive(Clone)]
//! struct Vector2 {
//!     x: f64,
//!     y: f64,
//! }
//!
//! #[guestjs::host_class(rename_all = "camelCase")]
//! impl Vector2 {
//!     #[guestjs(constructor)]
//!     fn new(
//!         x: f64,
//!         y: f64,
//!     ) -> Result<Self, Error> {
//!         Ok(
//!             Self {
//!                 x,
//!                 y,
//!             },
//!         )
//!     }
//!
//!     #[guestjs(method)]
//!     fn length(&self) -> Result<f64, Error> {
//!         Ok((self.x * self.x + self.y * self.y).sqrt())
//!     }
//!
//!     #[guestjs(method, name = "moveBy")]
//!     fn translate(
//!         &mut self,
//!         dx: f64,
//!         dy: f64,
//!     ) -> Result<(), Error> {
//!         self.x += dx;
//!         self.y += dy;
//!
//!         Ok(())
//!     }
//!
//!     #[guestjs(get)]
//!     fn x(&self) -> Result<f64, Error> {
//!         Ok(self.x)
//!     }
//!
//!     #[guestjs(set, name = "x")]
//!     fn set_x(&mut self, x: f64) -> Result<(), Error> {
//!         self.x = x;
//!
//!         Ok(())
//!     }
//!
//!     #[guestjs(iterable)]
//!     fn coordinates(&self) -> Result<[f64; 2], Error> {
//!         Ok([self.x, self.y])
//!     }
//!
//!     #[guestjs(symbol = "toPrimitive")]
//!     fn to_primitive(&self) -> Result<String, Error> {
//!         Ok(format!("Vector2({}, {})", self.x, self.y))
//!     }
//!
//!     #[guestjs(constant)]
//!     const DIMENSIONS: usize = 2;
//!
//!     #[guestjs(static_method)]
//!     fn zero_length() -> Result<f64, Error> {
//!         Ok(0.0)
//!     }
//!
//!     #[guestjs(statics)]
//!     fn add_statics(statics: &mut Namespace) {
//!         statics.constant("coordinateSystem", "cartesian");
//!     }
//!
//!     #[guestjs(async_method)]
//!     fn length_async(
//!         &self,
//!     ) -> Result<impl Future<Output = Result<f64, Error>> + 'static, Error> {
//!         let (x, y) = (self.x, self.y);
//!
//!         Ok(async move { Ok(x.hypot(y)) })
//!     }
//! }
//!
//! struct Geometry {
//!     expose_origin: bool,
//! }
//!
//! #[guestjs::host_module(
//!     name = "@host/geometry",
//!     classes(Vector2),
//!     rename_all = "camelCase",
//! )]
//! impl Geometry {
//!     #[guestjs(default)]
//!     const DEFAULT_SYSTEM: &'static str = "cartesian";
//!
//!     #[guestjs(constant)]
//!     const API_VERSION: i32 = 1;
//!
//!     #[guestjs(function)]
//!     fn hypot(
//!         left: f64,
//!         right: f64,
//!     ) -> Result<f64, Error> {
//!         Ok(left.hypot(right))
//!     }
//!
//!     #[guestjs(function)]
//!     async fn delayed_hypot(
//!         left: f64,
//!         right: f64,
//!     ) -> Result<f64, Error> {
//!         Ok(left.hypot(right))
//!     }
//!
//!     #[guestjs(object)]
//!     fn metadata(metadata: &mut Namespace) {
//!         metadata.constant("coordinateSystem", "cartesian");
//!         metadata.property("precision", 2);
//!     }
//!
//!     #[guestjs(init)]
//!     fn initialize(scope: &Scope<'_>) -> Result<(), Error> {
//!         scope
//!             .ctx()
//!             .globals()
//!             .set("__geometryReady", true)?;
//!
//!         Ok(())
//!     }
//!
//!     #[guestjs(build)]
//!     fn configure(
//!         &self,
//!         exports: &mut Exports,
//!     ) {
//!         if self.expose_origin {
//!             exports.constant("originAvailable", true);
//!         }
//!     }
//! }
//!
//! let runtime = Runtime::builder()
//!     .bind(
//!         HostLibrary::new()
//!             .with(
//!                 Geometry {
//!                     expose_origin: true,
//!                 },
//!             )
//!             .initialize(
//!                 HostInitializer::new("geometry:application", |scope| {
//!                     scope
//!                         .ctx()
//!                         .globals()
//!                         .set("__application", "geometry")?;
//!
//!                     Ok(())
//!                 }),
//!             ),
//!     )
//!     .build()
//!     .await?;
//!
//! assert_eq!(
//!     runtime
//!         .guest()
//!         .build()
//!         .await?
//!         .scope(async move |scope| {
//!             scope
//!                 .host_module("@host/geometry")?
//!                 .function("hypot")?
//!                 .call::<_, f64>((5.0, 12.0))
//!         })
//!         .await?,
//!     13.0,
//! );
//! assert_eq!(
//!     runtime
//!         .guest()
//!         .build()
//!         .await?
//!         .scope(async move |scope| {
//!             scope
//!                 .guest_module(
//!                     "geometry.js",
//!                     r#"
//! import defaultSystem, {
//!     Vector2,
//!     apiVersion,
//!     delayedHypot,
//!     hypot,
//!     metadata,
//!     originAvailable,
//! } from "@host/geometry";
//!
//! export async function describe() {
//!     const vector = new Vector2(3, 4);
//!
//!     vector.moveBy(1, 2);
//!     vector.x = 6;
//!     metadata.precision = 3;
//!
//!     return [
//!         defaultSystem,
//!         apiVersion,
//!         metadata.coordinateSystem,
//!         metadata.precision,
//!         originAvailable,
//!         vector.x,
//!         [...vector].join(","),
//!         String(vector),
//!         Vector2.dimensions,
//!         Vector2.zeroLength(),
//!         Vector2.coordinateSystem,
//!         await vector.lengthAsync(),
//!         hypot(5, 12),
//!         await delayedHypot(5, 12),
//!         globalThis.__geometryReady === true,
//!         globalThis.__application,
//!     ].join("|");
//! }
//! "#,
//!                 )
//!                 .await?
//!                 .function("describe")?
//!                 .call::<_, Promise<String>>(())?
//!                 .await
//!         })
//!         .await?,
//!     "cartesian|1|cartesian|3|true|6|6,6|Vector2(6, 6)|2|0|cartesian|8.48528137423857|13|13|true|geometry",
//! );
//! ```

#[allow(unused_extern_crates)]
extern crate self as guestjs;

pub mod prelude;

/// LLRT native-library support.
#[cfg(any(
    feature = "llrt-buffer",
    feature = "llrt-console",
    feature = "llrt-fetch",
    feature = "llrt-fs",
    feature = "llrt-os",
    feature = "llrt-process-env",
    feature = "llrt-streams",
    feature = "llrt-timers",
    feature = "llrt-url",
))]
pub mod llrt {
    #[cfg(feature = "llrt-streams")]
    pub use guestjs_llrt::streams;

    pub use guestjs_llrt::{Llrt, LlrtBuilder};
}

pub use guestjs_core::*;
pub use guestjs_macros::*;

#[cfg(test)]
mod tests {
    use std::{cell::Cell, future::Future, rc::Rc};

    use crate::{
        __private::JsValue,
        errors::Error,
        handle::{BoundFunction, Class, Function, Object, Promise},
        host::{Exports, Namespace},
        marshal::{FromGuestBound, Nullish},
        runtime::{Runtime, Scope},
    };

    #[derive(
        Debug, PartialEq, serde::Serialize, serde::Deserialize, crate::ToGuest, crate::FromGuest,
    )]
    struct Request {
        #[serde(rename = "userId")]
        user_id: u64,
        note: Option<String>,
    }

    #[derive(Debug, PartialEq, serde::Deserialize, crate::FromGuest)]
    #[serde(rename_all = "lowercase")]
    enum Status {
        Ready,
    }

    #[derive(Debug, PartialEq, serde::Deserialize, crate::FromGuest)]
    struct Response {
        #[serde(rename = "userId")]
        user_id: u64,
        status: Status,
    }

    #[derive(Debug, thiserror::Error)]
    #[error("host class failure")]
    struct HostClassError;

    impl From<HostClassError> for Error {
        fn from(error: HostClassError) -> Self {
            Error::unexpected(error.to_string())
        }
    }

    const GENERATED_HOST_MODULE_SOURCE: &str = r#"
import { MacroPoint, addValues, delayedProduct } from "@host/macro";

export async function exercise() {
    return [
        new MacroPoint(4).readValue(),
        addValues(2, 3),
        await delayedProduct(3, 4),
    ].join("|");
}
"#;

    const COMPLETE_HOST_MODULE_SOURCE: &str = r#"
import * as complete from "@host/complete";

export async function exercise() {
    const point = new complete.tools.MacroPoint(4);

    complete.tools.writable = 9;
    complete.tools.count = 7;

    return [
        complete.default,
        complete.apiVersion,
        complete.tools.add(2, 3),
        await complete.tools.delayedProduct(3, 4),
        complete.tools.writable,
        complete.tools.count,
        point.readValue(),
        "conditional" in complete,
        globalThis.__completeInitialized === true,
    ].join("|");
}
"#;

    const TYPED_GUEST_MODULE_SOURCE: &str = r#"
export function zero() {
    return 0;
}

export function add(left, right) {
    return left + right;
}

export function makeAdder(amount) {
    return value => value + amount;
}

export function apply(operation, value) {
    return operation(value);
}

export async function delayed(value) {
    return value * 2;
}

export function optional(value) {
    return value;
}

export function nullish(value) {
    return value;
}

export const answer = 42;
export let revision = 1;
export const optionalValue = null;
export const nullishValue = undefined;
export const settings = {
    unit: "px",
};

export class Counter {
    constructor(value) {
        this.value = value;
    }

    increment() {
        return ++this.value;
    }
}

export const operation = value => value + 1;
export const pendingOperation = Promise.resolve(value => value * 2);

export function advance() {
    revision += 1;
}
"#;

    crate::guest_module! {
        module TypedGuestModule {
            fn zero() -> i32;

            fn add(
                left: i32,
                right: i32,
            ) -> i32;

            #[guestjs(name = "makeAdder")]
            fn make_adder(
                amount: i32,
            ) -> Function;

            fn apply(
                operation: Function,
                value: i32,
            ) -> i32;

            fn delayed(
                value: i32,
            ) -> Promise<i32>;

            fn optional(
                value: Option<i32>,
            ) -> Option<i32>;

            fn nullish(
                value: Nullish<i32>,
            ) -> Nullish<i32>;

            fn advance() -> ();

            value answer: i32;
            value revision: i32;

            #[guestjs(name = "optionalValue")]
            value optional_value: Option<i32>;

            #[guestjs(name = "nullishValue")]
            value nullish_value: Nullish<i32>;

            value settings: Object;

            #[guestjs(name = "Counter")]
            value counter: Class;
            value operation: Function;

            #[guestjs(name = "pendingOperation")]
            value pending_operation: Promise<Function>;
        }
    }

    struct MacroPoint {
        value: i32,
    }

    #[crate::host_class(rename_all = "camelCase")]
    impl MacroPoint {
        #[guestjs(constructor)]
        fn new(value: i32) -> Result<Self, HostClassError> {
            Ok(Self { value })
        }

        #[guestjs(method)]
        fn read_value(&self) -> Result<i32, HostClassError> {
            Ok(self.value)
        }

        #[guestjs(method)]
        fn add(&mut self, value: i32) -> Result<i32, HostClassError> {
            self.value += value;

            Ok(self.value)
        }

        #[guestjs(method)]
        fn classify(
            &self,
            optional: Option<i32>,
            nullish: Nullish<i32>,
        ) -> Result<String, HostClassError> {
            Ok(format!("{optional:?}:{nullish:?}"))
        }

        #[guestjs(method)]
        fn combine(&self, #[guestjs(borrow)] other: &MacroPoint) -> Result<i32, HostClassError> {
            Ok(self.value + other.value)
        }

        #[guestjs(method)]
        fn transfer(
            &mut self,
            #[guestjs(borrow_mut)] other: &mut MacroPoint,
            amount: i32,
        ) -> Result<i32, HostClassError> {
            self.value -= amount;
            other.value += amount;

            Ok(other.value)
        }

        #[guestjs(method)]
        fn apply(
            &self,
            #[guestjs(as = Function)] callback: BoundFunction<'_>,
            value: i32,
        ) -> Result<i32, HostClassError> {
            callback
                .call::<_, i32>((value,))
                .map_err(|_error| HostClassError)
        }

        #[guestjs(method)]
        fn total(
            &self,
            first: i32,
            #[guestjs(rest)] values: Vec<i32>,
        ) -> Result<i32, HostClassError> {
            Ok(first + values.into_iter().sum::<i32>())
        }

        #[guestjs(method)]
        fn has_scope(&self, #[guestjs(scope)] _scope: &Scope<'_>) -> Result<bool, HostClassError> {
            Ok(true)
        }

        #[guestjs(method)]
        fn fail(&self) -> Result<(), HostClassError> {
            Err(HostClassError)
        }

        #[guestjs(get)]
        fn value(&self) -> Result<i32, HostClassError> {
            Ok(self.value)
        }

        #[guestjs(set, name = "value")]
        fn set_value(&mut self, value: i32) -> Result<(), HostClassError> {
            self.value = value;

            Ok(())
        }

        #[guestjs(iterable)]
        fn values(&self) -> Result<[i32; 2], HostClassError> {
            Ok([self.value, self.value + 1])
        }

        #[guestjs(symbol = "toPrimitive")]
        fn to_primitive(&self) -> Result<String, HostClassError> {
            Ok(format!("point:{}", self.value))
        }

        #[guestjs(constant)]
        const DIMENSIONS: i32 = 1;

        #[guestjs(static_method)]
        fn origin_value() -> Result<i32, HostClassError> {
            Ok(0)
        }

        #[guestjs(statics)]
        fn add_statics(statics: &mut Namespace) {
            statics.constant("hooked", 7);
        }

        #[guestjs(async_method)]
        fn double_async(
            &self,
        ) -> Result<impl Future<Output = Result<i32, HostClassError>> + 'static, HostClassError>
        {
            let value = self.value;

            Ok(async move { Ok(value * 2) })
        }

        #[guestjs(async_method)]
        fn add_async(
            &mut self,
            amount: i32,
        ) -> Result<impl Future<Output = Result<i32, HostClassError>> + 'static, HostClassError>
        {
            self.value += amount;

            let value = self.value;

            Ok(async move { Ok(value) })
        }
    }

    struct MacroHost;

    #[crate::host_module(name = "@host/macro", classes(MacroPoint), rename_all = "camelCase")]
    impl MacroHost {
        #[guestjs(function)]
        fn add_values(left: i32, right: i32) -> Result<i32, HostClassError> {
            Ok(left + right)
        }

        #[guestjs(function, name = "delayedProduct")]
        async fn multiply(left: i32, right: i32) -> Result<i32, HostClassError> {
            Ok(left * right)
        }
    }

    struct CompleteMacroHost {
        conditional: bool,
        count: Rc<Cell<i32>>,
    }

    #[crate::host_module(name = "@host/complete", classes(MacroPoint), rename_all = "camelCase")]
    impl CompleteMacroHost {
        #[guestjs(default)]
        const FALLBACK: &'static str = "fallback";

        #[guestjs(constant)]
        const API_VERSION: i32 = 2;

        #[guestjs(init)]
        fn init(scope: &Scope<'_>) -> Result<(), Error> {
            scope
                .ctx()
                .globals()
                .set("__completeInitialized", true)?;

            Ok(())
        }

        #[guestjs(object, name = "tools")]
        fn build_tools(&self, tools: &mut Namespace) {
            let getter_count = self.count.clone();
            let setter_count = self.count.clone();

            tools.function("add", |scope, args| {
                Ok(args.get::<i32>(scope, 0)? + args.get::<i32>(scope, 1)?)
            });
            tools.async_function("delayedProduct", |scope, args| {
                let left = args.get::<i32>(scope, 0)?;
                let right = args.get::<i32>(scope, 1)?;

                Ok(async move { Ok(left * right) })
            });
            tools.property("writable", 1);
            tools.accessor::<_, _, _, i32>(
                "count",
                move |_scope| Ok(getter_count.get()),
                move |_scope, value| {
                    setter_count.set(value);

                    Ok(())
                },
            );
            tools.class::<MacroPoint>();
        }

        #[guestjs(build)]
        fn add_conditional(&self, exports: &mut Exports) {
            if self.conditional {
                exports.constant("conditional", true);
            }
        }
    }

    #[tokio::test]
    async fn derived_values_roundtrip_through_guest_functions() {
        assert_eq!(
            Runtime::builder()
                .build()
                .await
                .unwrap()
                .guest()
                .build()
                .await
                .unwrap()
                .guest_module(
                    "request.js",
                    r#"
export function normalize(request) {
    return {
        userId: request.userId,
        note: request.note,
    };
}
"#,
                )
                .await
                .unwrap()
                .function("normalize")
                .await
                .unwrap()
                .call::<_, Request>((Request { user_id: 42, note: None },),)
                .await
                .unwrap(),
            Request { user_id: 42, note: None },
        );
    }

    #[tokio::test]
    async fn derived_values_deserialize_in_owned_and_bound_operations() {
        let guest = Runtime::builder()
            .build()
            .await
            .unwrap()
            .guest()
            .build()
            .await
            .unwrap();

        assert_eq!(
            guest
                .eval::<Response>(r#"({ userId: 42, status: "ready" })"#)
                .await
                .unwrap(),
            Response { user_id: 42, status: Status::Ready },
        );
        assert_eq!(
            guest
                .scope(async |scope| {
                    Response::from_guest_bound(
                        &scope,
                        scope
                            .ctx()
                            .eval::<JsValue, _>(r#"({ userId: 7, status: "ready" })"#)
                            .map_err(Error::from)?,
                    )
                })
                .await
                .unwrap(),
            Response { user_id: 7, status: Status::Ready },
        );
        assert_eq!(
            guest
                .eval::<Status>(r#""ready""#)
                .await
                .unwrap(),
            Status::Ready,
        );
    }

    #[tokio::test]
    async fn derived_conversion_preserves_deserialization_error_sources() {
        assert!(matches!(
            Runtime::builder()
                .build()
                .await
                .unwrap()
                .guest()
                .build()
                .await
                .unwrap()
                .eval::<Response>(r#"({ userId: "invalid", status: "ready" })"#)
                .await,
            Err(Error::Conversion { source: Some(_), .. }),
        ));
    }

    #[tokio::test]
    async fn generated_host_class_supports_typed_synchronous_methods() {
        let module = Runtime::builder()
            .bind(MacroHost)
            .build()
            .await
            .unwrap()
            .guest()
            .build()
            .await
            .unwrap()
            .guest_module(
                "host-class.js",
                r#"
import { MacroPoint } from "@host/macro";

export function exercise() {
    const left = new MacroPoint(4);
    const right = new MacroPoint(6);

    left.add(2);

    return [
        left.transfer(right, 2),
        left.readValue(),
        left.classify(),
        left.classify(null, null),
        left.combine(right),
        left.apply(value => value * 2, 5),
        left.total(1, 2, 3),
        left.hasScope(),
    ].join("|");
}

export function missing() {
    return new MacroPoint();
}

export function failure() {
    return new MacroPoint(1).fail();
}
"#,
            )
            .await
            .unwrap();

        assert_eq!(
            module
                .function("exercise")
                .await
                .unwrap()
                .call::<_, String>(())
                .await
                .unwrap(),
            "8|4|None:Undefined|None:Null|12|10|6|true",
        );
        assert!(
            module
                .function("missing")
                .await
                .unwrap()
                .call::<_, String>(())
                .await
                .is_err(),
        );
        assert!(
            module
                .function("failure")
                .await
                .unwrap()
                .call::<_, ()>(())
                .await
                .is_err(),
        );
    }

    #[tokio::test]
    async fn generated_host_class_supports_complete_class_members() {
        assert_eq!(
            Runtime::builder()
                .bind(MacroHost)
                .build()
                .await
                .unwrap()
                .guest()
                .build()
                .await
                .unwrap()
                .guest_module(
                    "complete-host-class.js",
                    r#"
import { MacroPoint } from "@host/macro";

export async function exercise() {
    const point = new MacroPoint(3);

    point.value = 5;

    const values = [...point].join(",");
    const doubled = await point.doubleAsync();
    const added = await point.addAsync(2);

    return [
        point.value,
        values,
        String(point),
        MacroPoint.dimensions,
        MacroPoint.originValue(),
        MacroPoint.hooked,
        doubled,
        added,
    ].join("|");
}
"#,
                )
                .await
                .unwrap()
                .function("exercise")
                .await
                .unwrap()
                .call::<_, Promise<String>>(())
                .await
                .unwrap()
                .await
                .unwrap(),
            "7|5,6|point:7|1|0|7|10|7",
        );
    }

    #[tokio::test]
    async fn generated_host_module_supports_runtime_bindings() {
        assert_eq!(
            Runtime::builder()
                .bind(MacroHost)
                .build()
                .await
                .unwrap()
                .guest()
                .build()
                .await
                .unwrap()
                .guest_module("runtime-host-module.js", GENERATED_HOST_MODULE_SOURCE,)
                .await
                .unwrap()
                .function("exercise")
                .await
                .unwrap()
                .call::<_, Promise<String>>(())
                .await
                .unwrap()
                .await
                .unwrap(),
            "4|5|12",
        );
    }

    #[tokio::test]
    async fn generated_host_module_supports_guest_bindings() {
        assert_eq!(
            Runtime::builder()
                .build()
                .await
                .unwrap()
                .guest()
                .bind(MacroHost)
                .build()
                .await
                .unwrap()
                .guest_module("guest-host-module.js", GENERATED_HOST_MODULE_SOURCE,)
                .await
                .unwrap()
                .function("exercise")
                .await
                .unwrap()
                .call::<_, Promise<String>>(())
                .await
                .unwrap()
                .await
                .unwrap(),
            "4|5|12",
        );
    }

    #[tokio::test]
    async fn generated_host_module_supports_complete_exports() {
        for (conditional, expected) in [
            (true, "fallback|2|5|12|9|7|4|true|true"),
            (false, "fallback|2|5|12|9|7|4|false|true"),
        ] {
            assert_eq!(
                Runtime::builder()
                    .bind(CompleteMacroHost {
                        conditional,
                        count: Rc::new(Cell::new(1)),
                    })
                    .build()
                    .await
                    .unwrap()
                    .guest()
                    .build()
                    .await
                    .unwrap()
                    .guest_module("complete-host-module.js", COMPLETE_HOST_MODULE_SOURCE,)
                    .await
                    .unwrap()
                    .function("exercise")
                    .await
                    .unwrap()
                    .call::<_, Promise<String>>(())
                    .await
                    .unwrap()
                    .await
                    .unwrap(),
                expected,
            );
        }
    }

    #[tokio::test]
    async fn generated_guest_module_supports_owned_and_bound_calls() {
        let guest = Runtime::builder()
            .build()
            .await
            .unwrap()
            .guest()
            .build()
            .await
            .unwrap();

        let module = TypedGuestModule::from(
            guest
                .guest_module("typed-guest-module.js", TYPED_GUEST_MODULE_SOURCE)
                .await
                .unwrap(),
        );

        assert_eq!(module.zero().await.unwrap(), 0);
        assert_eq!(module.add(20, 22).await.unwrap(), 42);
        assert_eq!(
            module
                .apply(module.make_adder(1).await.unwrap(), 41,)
                .await
                .unwrap(),
            42,
        );
        assert_eq!(module.optional(None).await.unwrap(), None);
        assert_eq!(
            module
                .nullish(Nullish::Undefined)
                .await
                .unwrap(),
            Nullish::Undefined,
        );
        assert_eq!(
            module
                .delayed(21)
                .await
                .unwrap()
                .await
                .unwrap(),
            42,
        );
        assert_eq!(module.answer().await.unwrap(), 42);
        assert_eq!(module.revision().await.unwrap(), 1);

        module.advance().await.unwrap();

        assert_eq!(module.revision().await.unwrap(), 2);
        assert_eq!(module.optional_value().await.unwrap(), None);
        assert_eq!(module.nullish_value().await.unwrap(), Nullish::Undefined,);
        assert_eq!(
            module
                .settings()
                .await
                .unwrap()
                .get::<String>("unit")
                .await
                .unwrap(),
            "px",
        );
        assert_eq!(
            module
                .counter()
                .await
                .unwrap()
                .construct((1,))
                .await
                .unwrap()
                .call::<_, i32>("increment", ())
                .await
                .unwrap(),
            2,
        );
        assert_eq!(
            module
                .operation()
                .await
                .unwrap()
                .call::<_, i32>((41,))
                .await
                .unwrap(),
            42,
        );
        assert_eq!(
            module
                .pending_operation()
                .await
                .unwrap()
                .await
                .unwrap()
                .call::<_, i32>((21,))
                .await
                .unwrap(),
            42,
        );

        guest
            .scope(async move |scope| {
                let module = module.bind(&scope)?;

                assert_eq!(module.add(1, 2)?, 3);
                assert_eq!(module.add(20, 22)?, 42);
                assert_eq!(module.apply(module.make_adder(1)?, 41,)?, 42,);
                assert_eq!(module.optional(None)?, None);
                assert_eq!(module.nullish(Nullish::Null)?, Nullish::Null,);
                assert_eq!(module.delayed(21)?.await?, 42);
                assert_eq!(module.answer()?, 42);
                assert_eq!(module.revision()?, 2);

                module.advance()?;

                assert_eq!(module.revision()?, 3);
                assert_eq!(module.optional_value()?, None);
                assert_eq!(module.nullish_value()?, Nullish::Undefined);
                assert_eq!(
                    module
                        .settings()?
                        .get::<String>("unit")?,
                    "px"
                );
                assert_eq!(
                    module
                        .counter()?
                        .construct((1,))?
                        .call::<_, i32>("increment", ())?,
                    2,
                );
                assert_eq!(
                    module
                        .operation()?
                        .call::<_, i32>((41,))?,
                    42
                );
                assert_eq!(
                    module
                        .pending_operation()?
                        .await?
                        .call::<_, i32>((21,))?,
                    42,
                );

                Ok(())
            })
            .await
            .unwrap();
    }
}
