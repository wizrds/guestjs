//! Lets a Rust application load and run JavaScript and TypeScript code, call the functions and
//! read the values that code defines, and expose Rust functions, classes, and modules for guest
//! code to call back into. This crate runs on a pluggable backend. The
//! [`crate::quickjs::QuickJs`](crate::quickjs::QuickJs) backend uses rquickjs and is available
//! through [`crate::quickjs::QuickJs`](crate::quickjs::QuickJs) with the `quickjs` Cargo
//! feature enabled.
//!
//! GuestJS types are generic over a backend type parameter. This crate reexports the core API
//! and macros so applications normally depend only on `guestjs`. Enabling `quickjs` reexports
//! [`guestjs_quickjs`](guestjs_quickjs) as [`crate::quickjs`](crate::quickjs).
//!
//! # Runtimes, guests, and isolation
//!
//! A [`guestjs_core::runtime::Runtime`](guestjs_core::runtime::Runtime) owns the selected
//! backend. Each call to
//! [`guestjs_core::runtime::Runtime::guest`](guestjs_core::runtime::Runtime::guest) builds a
//! fresh, isolated JavaScript context:
//!
//! ```ignore
//! use guestjs::prelude::*;
//! use guestjs::quickjs::QuickJs;
//!
//! let runtime = Runtime::<QuickJs>::builder()
//!     .build()?;
//!
//! let first = runtime
//!     .guest()
//!     .build()?;
//!
//! let second = runtime
//!     .guest()
//!     .build()?;
//!
//! first.exec("name = 'first'")?;
//!
//! second.exec("name = 'second'")?;
//!
//! assert_eq!(first.globals()?.get::<String, _>("name")?, "first");
//!
//! assert_eq!(second.globals()?.get::<String, _>("name")?, "second");
//! ```
//!
//! Bind host libraries, native libraries, and bundles to a runtime when all its guests should
//! receive them, or to a guest builder when they belong to one guest. Builders can deny imports
//! by name. Owned handles retain the guest needed for later operations. Closing a guest refuses
//! subsequent entries; final ownership release controls when its retained context can be
//! destroyed.
//!
//! # Execution control
//!
//! Configure a timeout and cancellation signal on the runtime when an application needs to stop
//! guest execution:
//!
//! ```ignore
//! use std::time::Duration;
//!
//! use guestjs::prelude::*;
//! use guestjs::quickjs::{Config, QuickJs};
//!
//! let cancellation = Cancellation::new();
//!
//! let runtime = Runtime::<QuickJs>::builder()
//!     .config(
//!         Config::default()
//!             .memory_limit(32 * 1024 * 1024),
//!     )
//!     .timeout(Duration::from_millis(50))
//!     .cancellation(cancellation.clone())
//!     .build()?;
//!
//! let guest = runtime
//!     .guest()
//!     .build()?;
//!
//! assert!(
//!     guest
//!         .exec("while (true) {}")
//!         .is_err_and(|error| error.is_timeout()),
//! );
//!
//! cancellation.cancel();
//!
//! assert!(
//!     guest
//!         .eval::<f64>("1 + 1")
//!         .is_err_and(|error| error.is_cancelled()),
//! );
//! ```
//!
//! A guest builder inherits the runtime's default policy and can override it with
//! [`guestjs_core::builder::GuestBuilder::policy`](guestjs_core::builder::GuestBuilder::policy).
//! Use the selected backend's configuration type with
//! [`guestjs_core::builder::RuntimeBuilder::config`](guestjs_core::builder::RuntimeBuilder::config)
//! for engine-specific settings.
//!
//! # Loading guest code
//!
//! Use [`guestjs_core::guest::Guest::guest_module`](guestjs_core::guest::Guest::guest_module)
//! to load a named JavaScript or TypeScript module and work with its exports:
//!
//! ```ignore
//! use guestjs::prelude::*;
//! use guestjs::quickjs::QuickJs;
//!
//! let guest = Runtime::<QuickJs>::builder()
//!     .build()?
//!     .guest()
//!     .build()?;
//!
//! let module = guest.guest_module(
//!     "dynamic.js",
//!     r#"
//! export const settings = { prefix: "hello" };
//!
//! export function greet(name) {
//!     return `${settings.prefix} ${name}`;
//! }
//! "#,
//! )?;
//!
//! assert_eq!(
//!     module.object("settings")?.get::<String, _>("prefix")?,
//!     "hello",
//! );
//!
//! assert_eq!(
//!     module.function("greet")?.call::<_, String>(("Ada",))?,
//!     "hello Ada",
//! );
//! ```
//!
//! This dynamic handle API is useful when the module shape is not known until runtime. Use
//! [`guestjs_core::handle::protocol::ObjectProtocol::call_method`](guestjs_core::handle::protocol::ObjectProtocol::call_method)
//! when the called function must receive the original object as `this`.
//!
//! ## Loading several modules together with Bundle
//!
//! Use [`guestjs_core::bundle::Bundle`](guestjs_core::bundle::Bundle) when guest code spans
//! several modules or includes data:
//!
//! ```ignore
//! use guestjs::prelude::*;
//! use guestjs::quickjs::QuickJs;
//!
//! let bundle = Bundle::builder()
//!     .module(
//!         "app.js",
//!         r#"
//! import { double } from "./app/util.js";
//!
//! export function run() {
//!     return double(21);
//! }
//! "#,
//!     )
//!     .module(
//!         "app/util.js",
//!         "export const double = value => value * 2;",
//!     )
//!     .data("app/settings.json", b"{}")
//!     .build()?;
//!
//! let guest = Runtime::<QuickJs>::builder()
//!     .build()?
//!     .guest()
//!     .build()?;
//!
//! let module = guest.load(&bundle)?;
//!
//! assert_eq!(module.function("run")?.call::<_, f64>(())?, 42.0);
//! ```
//!
//! [`guestjs_core::bundle::Bundle`](guestjs_core::bundle::Bundle) paths name files relative to
//! the source tree. JavaScript module names omit their script extensions. TypeScript is
//! transpiled when added to a bundle; no typescript feature is needed.
//! [`guestjs_core::bundle::Bundle`](guestjs_core::bundle::Bundle) construction does not infer
//! package.json or turn index.js into a parent module name.
//!
//! Enable `embedded` for [`crate::bundle`](crate::bundle) and
//! [`guestjs_core::bundle::Bundle::from_embedded`](guestjs_core::bundle::Bundle::from_embedded).
//! Enable `tokio` for
//! [`guestjs_core::bundle::Bundle::from_dir`](guestjs_core::bundle::Bundle::from_dir) and the
//! builder's
//! [`guestjs_core::bundle::BundleBuilder::directory`](guestjs_core::bundle::BundleBuilder::directory).
//! Embedded construction operates on in-memory bytes and stays synchronous. The selected
//! directory's basename is not added as a module prefix. The include_dir dependency's stable
//! embedding path does not guarantee incremental rebuild discovery of newly added files.
//!
//! # Typed guest facades
//!
//! When a guest module has a known interface, [`crate::guest_module`](crate::guest_module)
//! generates a typed Rust facade:
//!
//! ```ignore
//! use guestjs::prelude::*;
//! use guestjs::quickjs::QuickJs;
//!
//! guestjs::guest_module! {
//!     pub module Math {
//!         fn add(left: f64, right: f64) -> f64;
//!
//!         value answer: f64;
//!     }
//! }
//!
//! let guest = Runtime::<QuickJs>::builder()
//!     .build()?
//!     .guest()
//!     .build()?;
//!
//! let math = Math::from(
//!     guest.guest_module(
//!         "math.js",
//!         r#"
//! export function add(left, right) {
//!     return left + right;
//! }
//!
//! export const answer = 42;
//! "#,
//!     )?,
//! );
//!
//! assert_eq!(math.add(20.0, 22.0)?, 42.0);
//!
//! assert_eq!(math.answer()?, 42.0);
//! ```
//!
//! Declare the functions and values the host needs. The generated facade converts each result
//! into the descriptor type declared in the macro.
//!
//! ## Typed guest classes
//!
//! Use [`crate::guest_class`](crate::guest_class) when the host knows the interface of a
//! JavaScript class or instance:
//!
//! ```ignore
//! use guestjs::prelude::*;
//! use guestjs::quickjs::QuickJs;
//!
//! guestjs::guest_class! {
//!     pub class Client {
//!         fn get(path: String) -> Response<B>;
//!
//!         value prefix: String;
//!     }
//! }
//!
//! guestjs::guest_class! {
//!     pub class Response {
//!         fn status() -> f64;
//!     }
//! }
//!
//! guestjs::guest_module! {
//!     pub module Plugin {
//!         #[guestjs(name = "Client")]
//!         value client_class: Class<B, Client<B>>;
//!
//!         value default_client: Client<B>;
//!     }
//! }
//!
//! let guest = Runtime::<QuickJs>::builder()
//!     .build()?
//!     .guest()
//!     .build()?;
//!
//! let plugin = Plugin::from(
//!     guest.guest_module(
//!         "plugin.js",
//!         r#"
//! class Response {
//!     constructor(code) {
//!         this.code = code;
//!     }
//!
//!     status() {
//!         return this.code;
//!     }
//! }
//!
//! export class Client {
//!     constructor(prefix) {
//!         this.prefix = prefix;
//!     }
//!
//!     get(path) {
//!         return new Response((this.prefix + path).length);
//!     }
//! }
//!
//! export const default_client = new Client("default:");
//! "#,
//!     )?,
//! );
//!
//! let client = plugin
//!     .client_class()?
//!     .construct(("api:".to_owned(),))?;
//!
//! assert_eq!(client.prefix()?, "api:");
//!
//! assert_eq!(client.get("users".to_owned())?.status()?, 9.0);
//! ```
//!
//! A generated class facade wraps an
//! [`guestjs_core::handle::class::Instance<B>`](guestjs_core::handle::class::Instance) and
//! calls JavaScript methods normally. Use
//! [`guestjs_core::handle::object::Object`](guestjs_core::handle::object::Object) for
//! unrestricted dynamic access,
//! [`guestjs_core::handle::class::Instance<B>`](guestjs_core::handle::class::Instance) for a
//! dynamic instance view, and
//! [`guestjs_core::handle::class::Instance<B, C>`](guestjs_core::handle::class::Instance) when
//! the live object carries a checked host payload `C` that Rust must borrow.
//!
//! # Guest-side async
//!
//! Calling a JavaScript `async` function returns a promise. Convert it to
//! [`guestjs_core::handle::promise::Promise`](guestjs_core::handle::promise::Promise) and await
//! that value from an async Rust function to drive guest progress and produce the fulfilled
//! JavaScript return value as a `T`:
//!
//! ```ignore
//! use guestjs::prelude::*;
//! use guestjs::quickjs::QuickJs;
//!
//! let guest = Runtime::<QuickJs>::builder()
//!     .build()?
//!     .guest()
//!     .build()?;
//!
//! guest.exec(
//!     r#"
//! async function double(value) {
//!     return value * 2;
//! }
//! "#,
//! )?;
//!
//! let doubled = guest
//!     .globals()?
//!     .function("double")?
//!     .call::<_, Promise<_, f64>>((21.0,))?
//!     .await?;
//!
//! assert_eq!(doubled, 42.0);
//! ```
//!
//! A [`guestjs_core::handle::promise::Promise`](guestjs_core::handle::promise::Promise)
//! requires a promise result. GuestJS does not select an executor. Other host and guest
//! operations are synchronous; module evaluation and asynchronous iteration expose their own
//! explicit async operations.
//!
//! # Plain Rust data
//!
//! Derive [`crate::ToGuest`](crate::ToGuest) and [`crate::FromGuest`](crate::FromGuest) for
//! serde-compatible data under the `serde` Cargo feature:
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
//! ```
//!
//! The derives follow the type's serde representation. Standard Rust primitives, options,
//! collections, arrays, and tuples also cross the boundary directly. Use a host class when
//! guest code needs to retain Rust object identity rather than receive copied data.
//!
//! ## Union inputs
//!
//! Use `#[guestjs(union)]` with [`crate::FromGuest`](crate::FromGuest) when a host API accepts several
//! guest value types. Every variant must contain exactly one unnamed field:
//!
//! ```ignore
//! #[derive(guestjs::FromGuest, guestjs::ToGuest)]
//! #[guestjs(union)]
//! enum Identifier {
//!     Number(f64),
//!     Text(String),
//! }
//! ```
//!
//! [`guestjs_core::marshal::FromGuest`](guestjs_core::marshal::FromGuest) tries variants in
//! declaration order and selects the first conversion that succeeds. A conversion mismatch
//! continues to the next variant, while guest exceptions and other failures return immediately.
//! Mismatch descriptions combine and deduplicate the accepted type names. Each attempt sees the
//! same guest object, so effects of an earlier attempt are not rolled back.
//!
//! Use `#[guestjs(union, backend = B)]` with [`crate::FromGuest`](crate::FromGuest) when the
//! enum already declares `B` as its backend
//! parameter. Without `backend = B`, the derives introduce their own backend parameter. Union
//! derives require the `serde` feature as well.
//!
//! # Binary data
//!
//! JavaScript binary data lives in an `ArrayBuffer` and is read and written through views: typed
//! arrays and `DataView`. [`guestjs_core::handle::buffer::ArrayBuffer`](guestjs_core::handle::buffer::ArrayBuffer),
//! [`guestjs_core::handle::buffer::TypedArray`](guestjs_core::handle::buffer::TypedArray) and
//! [`guestjs_core::handle::buffer::DataView`](guestjs_core::handle::buffer::DataView) are live
//! handles to those guest objects. Writes go into guest memory and reads return a copy:
//!
//! ```ignore
//! use guestjs::prelude::*;
//! use guestjs::quickjs::QuickJs;
//!
//! let guest = Runtime::<QuickJs>::builder()
//!     .build()?
//!     .guest()
//!     .build()?;
//!
//! guest.exec(
//!     r#"
//! globalThis.pixels = new Uint8Array([1, 2, 3, 4]);
//! "#,
//! )?;
//!
//! let pixels = guest
//!     .globals()?
//!     .get::<Uint8Array<_>, _>("pixels")?;
//!
//! pixels.write(1, &[9, 9])?;
//!
//! assert_eq!(pixels.to_vec()?, [1, 9, 9, 4]);
//! assert_eq!(pixels.buffer()?.byte_length()?, 4);
//! ```
//!
//! A slice of a typed array element type becomes a new typed array of that type, so `&[u8]`
//! becomes a `Uint8Array` and `&[f64]` a `Float64Array`. `Vec<T>` and Rust arrays still become
//! JavaScript arrays. Under the `bytes` Cargo feature, `bytes::Bytes` becomes a `Uint8Array` and
//! is read from any `ArrayBuffer`, typed array or `DataView`. Serde byte data under the `serde`
//! feature follows the same rules. A `SharedArrayBuffer` and views over one are not accepted.
//!
//! # Host classes and modules
//!
//! [`crate::host_class`](crate::host_class) exposes an ordinary Rust type as a JavaScript
//! class. Mark the constructor, methods, and properties guest code needs:
//!
//! ```ignore
//! use guestjs::prelude::*;
//!
//! struct Vector2 {
//!     x: f64,
//!     y: f64,
//! }
//!
//! #[guestjs::host_class]
//! impl Vector2 {
//!     #[guestjs(constructor)]
//!     fn new(x: f64, y: f64) -> Result<Self, Error> {
//!         Ok(Self { x, y })
//!     }
//!
//!     #[guestjs(method)]
//!     fn length(&self) -> Result<f64, Error> {
//!         Ok(self.x.hypot(self.y))
//!     }
//!
//!     #[guestjs(get)]
//!     fn x(&self) -> Result<f64, Error> {
//!         Ok(self.x)
//!     }
//! }
//! ```
//!
//! Guest code uses the generated class through ordinary JavaScript construction and property
//! access:
//!
//! ```javascript
//! const vector = new Vector2(3, 4);
//!
//! console.log(vector.x);
//!
//! console.log(vector.length());
//! ```
//!
//! Returning a host-class value to Rust preserves the live guest instance rather than cloning
//! the Rust payload. Use
//! [`guestjs_core::handle::class::Instance::borrow_with`](guestjs_core::handle::class::Instance::borrow_with)
//! or
//! [`guestjs_core::handle::class::Instance::borrow_with_mut`](guestjs_core::handle::class::Instance::borrow_with_mut)
//! when the host needs direct payload access; ordinary facade calls still use JavaScript
//! dispatch.
//!
//! The macro also supports mutable methods, constructor-side members, supported well-known
//! symbols, and asynchronous host work. A Rust `&self` or `&mut self` receiver accesses the
//! instance's Rust payload; an injected `this` parameter accesses its JavaScript object.
//!
//! A host class that stores guest values rather than plain Rust data is generic over the
//! backend. Name the backend parameter with `backend` so the macro knows which one it is:
//!
//! ```ignore
//! use guestjs::prelude::*;
//!
//! struct Envelope<B: Backend> {
//!     payload: Object<B>,
//! }
//!
//! #[guestjs::host_class(backend = B)]
//! impl<B: Backend> Envelope<B> {
//!     #[guestjs(constructor)]
//!     fn new(payload: Object<B>) -> Result<Self, Error> {
//!         Ok(Self { payload })
//!     }
//!
//!     #[guestjs(get)]
//!     fn payload(&self) -> Result<Object<B>, Error> {
//!         Ok(self.payload.clone())
//!     }
//! }
//!
//! struct Mail;
//!
//! #[guestjs::host_module(name = "host_mail", classes(Envelope<B>))]
//! impl Mail {}
//! ```
//!
//! A host class does not have to carry the backend in its own type. Naming a parameter the impl
//! does not declare introduces it on the exported members:
//!
//! ```ignore
//! struct Contract;
//!
//! #[guestjs::host_class(backend = B)]
//! impl Contract {
//!     #[guestjs(method)]
//!     fn active(#[guestjs(this)] this: &Object<B>) -> Result<bool, Error>
//!     where
//!         B: BackendValues,
//!     {
//!         this.has("active")
//!     }
//! }
//! ```
//!
//! In both forms `backend` names a type parameter. Whether it is reused from the impl or
//! introduced on the members is decided by the impl. Use `backend(pin = QuickJs)` to pin a
//! class to the [`crate::quickjs::QuickJs`](crate::quickjs::QuickJs) backend.
//! [`guestjs_core::host::class::HostClass`](guestjs_core::host::class::HostClass) and
//! [`guestjs_core::host::class::ClassDeclaration`](guestjs_core::host::class::ClassDeclaration)
//! remain available for handwritten declarations.
//!
//! [`crate::host_module`](crate::host_module) exposes Rust functions and classes through a
//! JavaScript-importable module:
//!
//! ```ignore
//! use guestjs::prelude::*;
//! use guestjs::quickjs::QuickJs;
//!
//! struct Geometry;
//!
//! #[guestjs::host_module(name = "host_geometry")]
//! impl Geometry {
//!     #[guestjs(function)]
//!     fn hypot(left: f64, right: f64) -> Result<f64, Error> {
//!         Ok(left.hypot(right))
//!     }
//! }
//!
//! let runtime = Runtime::<QuickJs>::builder()
//!     .bind(Geometry::module::<QuickJs>())
//!     .build()?;
//! ```
//!
//! Guest JavaScript code imports the generated exports normally:
//!
//! ```javascript
//! import { hypot } from "host_geometry";
//!
//! console.log(hypot(5, 12));
//! ```
//!
//! Bind one module directly to a runtime or guest builder according to the scope that should
//! receive it. Use
//! [`guestjs_core::host::library::HostLibrary`](guestjs_core::host::library::HostLibrary) when
//! that scope needs more than one host module or initializer.
//!
//! Module state is constructed per realisation and captured into callbacks. `context`, `enter`,
//! and `state` request the
//! [`guestjs_core::host::context::CallContext`](guestjs_core::host::context::CallContext), its
//! borrowed [`guestjs_core::enter::Enter`](guestjs_core::enter::Enter), and typed
//! [`guestjs_core::host::state::ModuleState`](guestjs_core::host::state::ModuleState)
//! respectively. Handwritten callbacks can use the same
//! [`guestjs_core::host::context::FromContext`](guestjs_core::host::context::FromContext) API
//! without macros.
//!
//! Native libraries are the backend-specific escape hatch for integrations that cannot be
//! expressed as host declarations. Bind native modules directly or group modules and
//! initializers in
//! [`guestjs_core::host::native::NativeLibrary`](guestjs_core::host::native::NativeLibrary).
//! They are separate from source bundles and require the selected backend's native integration
//! API.
//!
//! [`guestjs_core::guest::Guest::enter`](guestjs_core::guest::Guest::enter) provides the
//! current [`guestjs_core::enter::Enter`](guestjs_core::enter::Enter) for lower-level work
//! outside a host callback:
//!
//! ```ignore
//! let answer = guest.enter(|enter| {
//!     f64::from_guest(
//!         enter,
//!         QuickJs::eval(enter.token(), "6 * 7", "expression.js")?,
//!     )
//! })?;
//!
//! assert_eq!(answer, 42.0);
//! ```
//!
//! It follows the guest's execution policy. Owned handles retain the guest for later reentry;
//! direct entry does not create a second bound-handle API or allow a borrowed engine value to
//! escape.
//!
//! # Errors
//!
//! Fallible guest operations return
//! [`std::result::Result<T, guestjs_core::errors::Error>`](std::result::Result). Match a guest
//! exception when the application can recover from it, and propagate other failures:
//!
//! ```ignore
//! use guestjs::prelude::*;
//! use guestjs::quickjs::QuickJs;
//!
//! fn read_count(guest: &Guest<QuickJs>) -> Result<Option<f64>, Error> {
//!     match guest.eval::<f64>("JSON.parse('invalid')") {
//!         Ok(count) => Ok(Some(count)),
//!         Err(Error::Guest(exception)) if exception.matches("SyntaxError") => {
//!             eprintln!(
//!                 "guest rejected the count: {}",
//!                 exception.message(),
//!             );
//!
//!             Ok(None)
//!         }
//!         Err(error) => Err(error),
//!     }
//! }
//! ```
//!
//! [`guestjs_core::errors::GuestException::matches`](guestjs_core::errors::GuestException::matches)
//! recognizes recorded constructor names on the exception's prototype chain. Use
//! [`guestjs_core::errors::GuestException::name`](guestjs_core::errors::GuestException::name),
//! [`guestjs_core::errors::GuestException::message`](guestjs_core::errors::GuestException::message),
//! and optional
//! [`guestjs_core::errors::GuestException::stack`](guestjs_core::errors::GuestException::stack)
//! when reporting a guest failure. JavaScript can throw values that are not
//! JavaScript Error objects. Conversion, import,
//! borrowing, engine, interruption, timeout, cancellation, and closed-guest failures retain
//! their own error categories.
//!
//! Host callables can return application errors that convert into
//! [`guestjs_core::errors::Error`](guestjs_core::errors::Error). For example, with `thiserror`
//! as an application dependency:
//!
//! ```ignore
//! #[derive(Debug, thiserror::Error)]
//! #[error("geometry operation failed")]
//! struct GeometryError;
//!
//! impl From<GeometryError> for Error {
//!     fn from(error: GeometryError) -> Self {
//!         Error::sourced_unexpected(error.to_string(), error)
//!     }
//! }
//! ```
//!
//! Derive [`crate::HostException`](crate::HostException), register the exception with the host
//! module, and throw it using the active entry context:
//!
//! ```ignore
//! use guestjs::prelude::*;
//!
//! #[derive(HostException)]
//! #[guestjs(builtin = "TypeError")]
//! struct InvalidInput;
//!
//! struct Validation;
//!
//! #[guestjs::host_module(
//!     name = "host_validation",
//!     backend = B,
//!     exceptions(InvalidInput),
//! )]
//! impl Validation {
//!     #[guestjs(function)]
//!     fn check(
//!         #[guestjs(enter)] enter: &Enter<'_, B>,
//!         value: String,
//!     ) -> Result<(), Error>
//!     where
//!         B: BackendClasses + BackendExceptions,
//!     {
//!         if value.is_empty() {
//!             return Err(InvalidInput::throw(enter, "value must not be empty"));
//!         }
//!
//!         Ok(())
//!     }
//! }
//! ```
//!
//! JavaScript receives the registered error class through its normal exception model:
//!
//! ```javascript
//! import { check, InvalidInput } from "host_validation";
//!
//! try {
//!     check("");
//! } catch (error) {
//!     if (!(error instanceof InvalidInput)) {
//!         throw error;
//!     }
//!
//!     const message = error.message;
//! }
//! ```
//!
//! If JavaScript does not catch the exception, Rust receives it as
//! [`guestjs_core::errors::Error::Guest`](guestjs_core::errors::Error::Guest). Match the typed
//! exception without re-entering the guest:
//!
//! ```ignore
//! match guest
//!     .host_module("host_validation")?
//!     .function("check")?
//!     .call::<_, ()>(("",))
//! {
//!     Ok(()) => {},
//!     Err(error) if InvalidInput::caught(&error) => {
//!         eprintln!("invalid input: {error}");
//!     },
//!     Err(Error::Guest(exception)) if exception.matches("RangeError") => {
//!         eprintln!("out of range: {}", exception.message());
//!     },
//!     Err(error) => return Err(error),
//! }
//! ```
//!
//! [`guestjs_core::host::exception::HostException::caught`](guestjs_core::host::exception::HostException::caught)
//! checks the exception's recorded name hierarchy. Host exceptions expose a name, a built-in
//! base, and a message. Supported bases are
//! the JavaScript built-ins Error, TypeError, RangeError, ReferenceError, and SyntaxError.

#[allow(unused_extern_crates)]
extern crate self as guestjs;

pub mod prelude;

pub use guestjs_core::*;
pub use guestjs_macros::*;

#[cfg(feature = "quickjs")]
pub mod quickjs {
    pub use guestjs_quickjs::*;
}
