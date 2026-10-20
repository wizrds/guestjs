# GuestJS

GuestJS lets a Rust application load and run JavaScript and TypeScript code, call the functions and
read the values that code defines, and expose Rust functions, classes, and modules for guest code to
call back into. GuestJS runs on a pluggable backend. The `QuickJs` backend uses rquickjs, while the core
API and host declarations are generic over the selected backend.

The library provides:

- isolated guests sharing one runtime;
- direct Rust-to-JavaScript and JavaScript-to-Rust calls;
- typed guest-module facades;
- `Bundle` source trees;
- serde-backed conversion for ordinary Rust data;
- macros for defining Rust host classes and host modules;
- guest promises;
- timeouts and cancellation controls; and
- runtime-global and guest-local capability binding.

Applications choose guest source and decide which host and native capabilities each guest receives.
GuestJS does not install JavaScript packages or provide complete Node.js compatibility.

## Installation

Add GuestJS and select its `QuickJs` backend:

```toml
[dependencies]
guestjs = { git = "https://github.com/wizrds/guestjs", features = ["quickjs"] }
```

The backend is reexported through guestjs, so ordinary embedding does not require a separate
guestjs-quickjs dependency. There are no default features.

| Feature | Behavior |
| --- | --- |
| `quickjs` | Enables the `QuickJs` backend through `guestjs::quickjs`. |
| `embedded` | Enables embedded source/data bundles. |
| `serde` | Enables serde-backed conversion and the `ToGuest`/`FromGuest` derives. |
| `tokio` | Enables asynchronous filesystem bundle construction. |

Ordinary guest operations are synchronous. An application chooses its own executor when awaiting
guest promises or other asynchronous work. The `tokio` feature is not needed for embedded bundles or
ordinary guest calls.

## Quick start

Build a runtime, build a guest, load JavaScript source, and call an exported function:

```rust
use guestjs::prelude::*;
use guestjs::quickjs::QuickJs;

fn main() -> Result<(), Error> {
    let guest = Runtime::<QuickJs>::builder()
        .build()?
        .guest()
        .build()?;

    let module = guest.guest_module(
        "math.js",
        r#"
export function add(left, right) {
    return left + right;
}
"#,
    )?;

    let sum = module
        .get::<Function<_>, _>("add")?
        .call::<f64, _, _>((), (20.0, 22.0))?;

    assert_eq!(sum, 42.0);

    Ok(())
}
```

`Runtime::<QuickJs>::builder()` selects `QuickJs`. The calls in this example are synchronous;
GuestJS converts the arguments and result at the boundary. `Function::call` takes a receiver and
positional arguments; `()` supplies JavaScript `undefined` as the receiver.

## Runtimes, guests, and isolation

A `Runtime<B>` owns the selected backend. Each call to `Runtime::guest` builds a fresh, isolated
JavaScript context:

```rust
use guestjs::prelude::*;
use guestjs::quickjs::QuickJs;

let runtime = Runtime::<QuickJs>::builder()
    .build()?;

let first = runtime
    .guest()
    .build()?;

let second = runtime
    .guest()
    .build()?;

let first_module = first.guest_module(
    "identity.js",
    r#"
globalThis.name = "first";

export const name = globalThis.name;
"#,
)?;

let second_module = second.guest_module(
    "identity.js",
    r#"
globalThis.name = "second";

export const name = globalThis.name;
"#,
)?;

assert_eq!(first_module.get::<String, _>("name")?, "first");

assert_eq!(second_module.get::<String, _>("name")?, "second");
```

Bind host libraries, native libraries, and bundles to a runtime when all its guests should receive
them, or to a guest builder when they belong to one guest. Builders can deny imports by name.
Owned handles retain the guest needed for later operations. Closing a guest refuses subsequent
entries; final ownership release controls when its retained context can be destroyed.

### Execution control

Configure a timeout and cancellation signal on the runtime when an application needs to stop guest
execution:

```rust
use std::time::Duration;

use guestjs::prelude::*;
use guestjs::quickjs::{Config, QuickJs};

let cancellation = Cancellation::default();

let runtime = Runtime::<QuickJs>::builder()
    .config(
        Config::default()
            .memory_limit(32 * 1024 * 1024),
    )
    .policy(
        ExecutionPolicy::new()
            .timeout(Duration::from_millis(50))
            .cancellation(cancellation.clone()),
    )
    .build()?;

let guest = runtime
    .guest()
    .build()?;

assert!(
    guest
        .enter(|enter| {
            QuickJs::eval(enter.token(), "while (true) {}", "loop.js")
                .map(drop)
        })
        .is_err_and(|error| error.is_timeout()),
);

cancellation.cancel();

assert!(
    guest
        .enter(|_| Ok(()))
        .is_err_and(|error| error.is_cancelled()),
);
```

A guest builder inherits the runtime's default policy and can override it with `policy`. Use the
selected backend's configuration type with `RuntimeBuilder::config` for engine-specific settings.

## Loading guest code

Use `Guest::guest_module(path, source)` to load a named JavaScript or TypeScript module and work
with its exports:

```rust
use guestjs::prelude::*;
use guestjs::quickjs::QuickJs;

let guest = Runtime::<QuickJs>::builder()
    .build()?
    .guest()
    .build()?;

let module = guest.guest_module(
    "dynamic.js",
    r#"
export const settings = { prefix: "hello" };

export function greet(name) {
    return `${settings.prefix} ${name}`;
}
"#,
)?;

assert_eq!(
    module
        .get::<Object<_>, _>("settings")?
        .get::<String, _>("prefix")?,
    "hello",
);

assert_eq!(
    module
        .get::<Function<_>, _>("greet")?
        .call::<String, _, _>((), ("Ada",))?,
    "hello Ada",
);
```

This dynamic handle API is useful when the module shape is not known until runtime. Use
`ObjectProtocol::call_method` when the called function must receive the original object as `this`.

### Loading several modules together with Bundle

Use `Bundle` when guest code spans several modules or includes data:

```rust
use guestjs::prelude::*;
use guestjs::quickjs::QuickJs;

let bundle = Bundle::builder()
    .module(
        "app.js",
        r#"
import { double } from "./app/util.js";

export function run() {
    return double(21);
}
"#,
    )
    .module(
        "app/util.js",
        "export const double = value => value * 2;",
    )
    .data("app/settings.json", b"{}")
    .build()?;

let guest = Runtime::<QuickJs>::builder()
    .build()?
    .guest()
    .build()?;

let module = guest.load(&bundle)?;

assert_eq!(
    module
        .get::<Function<_>, _>("run")?
        .call::<f64, _, _>((), ())?,
    42.0,
);
```

`Bundle` paths name files relative to the source tree. JavaScript module names omit their script
extensions. TypeScript is transpiled when added to a bundle; no typescript feature is needed.
`Bundle` construction does not infer package.json or turn index.js into a parent module name.

Enable `embedded` for `guestjs::bundle!("$CARGO_MANIFEST_DIR/guest")` and
`Bundle::from_embedded`. Enable `tokio` for `Bundle::from_dir(path).await` and the builder's
`directory(path).await`. Embedded construction operates on in-memory bytes and stays synchronous.
The selected directory's basename is not added as a module prefix. The include_dir dependency's
stable embedding path does not guarantee incremental rebuild discovery of newly added files.

## Typed guest facades

When a guest module has a known interface, `guestjs::guest_module!` generates a typed Rust facade:

```rust
use guestjs::prelude::*;
use guestjs::quickjs::QuickJs;

guestjs::guest_module! {
    pub module Math {
        fn add(left: f64, right: f64) -> f64;

        value answer: f64;
    }
}

let guest = Runtime::<QuickJs>::builder()
    .build()?
    .guest()
    .build()?;

let math = Math::new(
    guest.guest_module(
        "math.js",
        r#"
export function add(left, right) {
    return left + right;
}

export const answer = 42;
"#,
    )?,
);

assert_eq!(math.add(20.0, 22.0)?, 42.0);

assert_eq!(math.answer()?, 42.0);
```

Declare the functions and values the host needs. The generated facade converts each result into the
descriptor type declared in the macro.

### Typed guest classes

Use `guestjs::guest_class!` when the host knows the interface of a JavaScript class or instance:

```rust
use guestjs::prelude::*;
use guestjs::quickjs::QuickJs;

guestjs::guest_class! {
    pub class Client {
        fn get(path: String) -> Response<B>;

        value prefix: String;
    }
}

guestjs::guest_class! {
    pub class Response {
        fn status() -> f64;
    }
}

guestjs::guest_module! {
    pub module Plugin {
        #[guestjs(name = "Client")]
        value client_class: Class<B, Client<B>>;

        value default_client: Client<B>;
    }
}

let guest = Runtime::<QuickJs>::builder()
    .build()?
    .guest()
    .build()?;

let plugin = Plugin::new(
    guest.guest_module(
        "plugin.js",
        r#"
class Response {
    constructor(code) {
        this.code = code;
    }

    status() {
        return this.code;
    }
}

export class Client {
    constructor(prefix) {
        this.prefix = prefix;
    }

    get(path) {
        return new Response((this.prefix + path).length);
    }
}

export const default_client = new Client("default:");
"#,
    )?,
);

let client = plugin
    .client_class()?
    .construct(("api:".to_owned(),))?;

assert_eq!(client.prefix()?, "api:");

assert_eq!(client.get("users".to_owned())?.status()?, 9.0);
```

A generated class facade wraps an `Instance<B>` and calls JavaScript methods normally. Use
`Object<B>` for unrestricted dynamic access, `Instance<B>` for a dynamic instance view, and
`Instance<B, C>` when the live object carries a checked host payload `C` that Rust must borrow.

## Guest-side async

Calling a JavaScript `async` function returns a promise. Convert it to `Promise<B, T>` and await
that value from an async Rust function to drive guest progress and produce the fulfilled JavaScript
return value as a `T`:

```rust
use guestjs::prelude::*;
use guestjs::quickjs::QuickJs;

#[tokio::main]
async fn main() -> Result<(), Error> {
    let guest = Runtime::<QuickJs>::builder()
        .build()?
        .guest()
        .build()?;

    let module = guest.guest_module(
        "async.js",
        r#"
export async function double(value) {
    return value * 2;
}
"#,
    )?;

    let doubled = module
        .get::<Function<_>, _>("double")?
        .call::<Promise<_, f64>, _, _>((), (21.0,))?
        .await?;

    assert_eq!(doubled, 42.0);

    Ok(())
}
```

This example uses Tokio as the application's executor, with its `macros` and `rt-multi-thread`
features enabled. GuestJS does not select an executor. A `Promise<B, T>` requires a promise result.
Other host and guest operations are synchronous; module evaluation and asynchronous iteration expose
their own explicit async operations.

## Plain Rust data

Derive `ToGuest` and `FromGuest` for ordinary serde-compatible Rust structs and enums under the
`serde` feature. The derives follow the type's `serde::Serialize` and
`serde::de::DeserializeOwned` implementations:

```toml
[dependencies]
guestjs = { git = "https://github.com/wizrds/guestjs", features = ["quickjs", "serde"] }
serde = { version = "1", features = ["derive"] }
```

```rust
use guestjs::prelude::*;

#[derive(
    Debug,
    PartialEq,
    serde::Serialize,
    serde::Deserialize,
    guestjs::ToGuest,
    guestjs::FromGuest,
)]
struct Request {
    #[serde(rename = "userId")]
    user_id: u64,
    note: Option<String>,
}
```

The derives follow the type's serde representation. Standard Rust primitives, options, collections,
arrays, and tuples also cross the boundary directly. Use a host class when guest code needs to retain
Rust object identity rather than receive copied data.

### Union inputs

Mark an enum with `#[guestjs(union)]` when a host API accepts several guest value types. Every
variant must contain exactly one unnamed field:

```rust
#[derive(guestjs::FromGuest, guestjs::ToGuest)]
#[guestjs(union)]
enum Identifier {
    Number(f64),
    Text(String),
}
```

`FromGuest` tries variants in declaration order and selects the first conversion that succeeds.
A conversion mismatch continues to the next variant, while guest exceptions and other failures
return immediately. Mismatch descriptions combine and deduplicate the accepted type names.
Each attempt sees the same guest object, so effects of an earlier attempt are not rolled back.

Use `#[guestjs(union, backend = B)]` when the enum already declares `B` as its backend parameter.
Without `backend = B`, the derives introduce their own backend parameter. Union derives require
the `serde` feature as well.

## Host classes

`#[guestjs::host_class]` exposes an ordinary Rust type as a JavaScript class. Mark the constructor,
methods, and properties guest code needs:

```rust
use guestjs::prelude::*;

struct Vector2 {
    x: f64,
    y: f64,
}

#[guestjs::host_class]
impl Vector2 {
    #[guestjs(constructor)]
    fn new(x: f64, y: f64) -> Result<Self, Error> {
        Ok(Self { x, y })
    }

    #[guestjs(method)]
    fn length(&self) -> Result<f64, Error> {
        Ok(self.x.hypot(self.y))
    }

    #[guestjs(get)]
    fn x(&self) -> Result<f64, Error> {
        Ok(self.x)
    }
}
```

Guest code uses the generated class through ordinary JavaScript construction and property access:

```javascript
const vector = new Vector2(3, 4);

console.log(vector.x);

console.log(vector.length());
```

Returning a host-class value to Rust preserves the live guest instance rather than cloning the Rust
payload. Use `Instance<B, Vector2>::borrow_with` or `borrow_with_mut` when the host needs direct
payload access; ordinary facade calls still use JavaScript dispatch.

The macro also supports mutable methods, constructor-side members, supported well-known symbols,
and asynchronous host work. A Rust `&self` or `&mut self` receiver accesses the instance's Rust payload;
an injected `this` parameter accesses its JavaScript object.

## Backend-generic host classes

A host class that stores guest values rather than plain Rust data is generic over the backend. Name
the backend parameter with `backend` so the macro knows which one it is:

```rust
use guestjs::prelude::*;

struct Envelope<B: Backend> {
    payload: Object<B>,
}

#[guestjs::host_class(backend = B)]
impl<B: Backend> Envelope<B> {
    #[guestjs(constructor)]
    fn new(payload: Object<B>) -> Result<Self, Error> {
        Ok(Self { payload })
    }

    #[guestjs(get)]
    fn payload(&self) -> Result<Object<B>, Error> {
        Ok(self.payload.clone())
    }
}
```

Register it like any other class, naming the parameter the generated module method is generic over:

```rust
struct Mail;

#[guestjs::host_module(name = "host_mail", classes(Envelope<B>))]
impl Mail {}
```

A host class does not have to carry the backend in its own type. Naming a parameter the impl does
not declare introduces it on the exported members:

```rust
struct Contract;

#[guestjs::host_class(backend = B)]
impl Contract {
    #[guestjs(method)]
    fn active(#[guestjs(this)] this: &Object<B>) -> Result<bool, Error>
    where
        B: BackendValues,
    {
        this.has("active")
    }
}
```

In both forms `backend` names a type parameter. Whether it is reused from the impl or introduced on
the members is decided by the impl. Use `backend(pin = QuickJs)` to pin a class to the `QuickJs`
backend. `HostClass` and `ClassDeclaration` remain available for handwritten declarations.

## Host modules

`#[guestjs::host_module]` exposes Rust functions and classes through a JavaScript-importable module:

```rust
use guestjs::prelude::*;
use guestjs::quickjs::QuickJs;

struct Geometry;

#[guestjs::host_module(name = "host_geometry")]
impl Geometry {
    #[guestjs(function)]
    fn hypot(left: f64, right: f64) -> Result<f64, Error> {
        Ok(left.hypot(right))
    }
}

let runtime = Runtime::<QuickJs>::builder()
    .bind(Geometry::module::<QuickJs>())
    .build()?;
```

Guest JavaScript code imports the generated exports normally:

```javascript
import { hypot } from "host_geometry";

console.log(hypot(5, 12));
```

Bind one module directly to a runtime or guest builder according to the scope that should receive
it. Use `HostLibrary` when that scope needs more than one host module or initializer.

Module state is constructed per realisation and captured into callbacks. `context`, `enter`, and
`state` request the `CallContext`, its borrowed `Enter`, and typed `ModuleState` respectively.
Handwritten callbacks can use the same `FromContext` API without macros.

## Native libraries

Native libraries are the backend-specific escape hatch for integrations that cannot be expressed
as host declarations. This advanced `QuickJs` example needs a compatible direct rquickjs dependency
because it implements that dependency's `ModuleDef`:

```rust
use guestjs::prelude::*;
use guestjs::quickjs::{
    backend::library::NativeModule as QuickJsNativeModule,
    QuickJs,
};
use rquickjs::{
    module::{
        Declarations,
        Exports,
        ModuleDef,
    },
    Ctx,
    Result as JsResult,
};

struct Environment;

impl ModuleDef for Environment {
    fn declare<'js>(declarations: &Declarations<'js>) -> JsResult<()> {
        declarations.declare("runtime").map(|_| ())
    }

    fn evaluate<'js>(
        _ctx: &Ctx<'js>,
        exports: &Exports<'js>,
    ) -> JsResult<()> {
        exports.export("runtime", "guestjs").map(|_| ())
    }
}

let runtime = Runtime::<QuickJs>::builder()
    .bind_native(NativeModule::<QuickJs>::new(
        "host_environment",
        QuickJsNativeModule::new::<Environment>(),
    ))
    .build()?;
```

Use rquickjs 0.14 with full-async enabled to match this backend. Bind a native module directly, or
combine native modules and initializers in `NativeLibrary`. Native code is not loaded from source
bundles, and binding native modules does not grant complete Node.js compatibility.

## Direct guest entry

`Guest::enter` provides the current `Enter` for lower-level work outside a host callback:

```rust
let answer = guest.enter(|enter| {
    f64::from_guest(
        enter,
        QuickJs::eval(enter.token(), "6 * 7", "expression.js")?,
    )
})?;

assert_eq!(answer, 42.0);
```

It follows the guest's execution policy. Owned handles retain the guest for later reentry; direct
entry does not create a second bound-handle API or allow a borrowed engine value to escape.

## Errors

Fallible guest operations return `Result<T, Error>`. Match a guest exception when the application can
recover from it, and propagate other failures:

```rust
use guestjs::prelude::*;
use guestjs::quickjs::QuickJs;

struct CountReader;

impl CountReader {
    fn read(guest: &Guest<QuickJs>) -> Result<Option<f64>, Error> {
        match guest.enter(|enter| {
            f64::from_guest(
                enter,
                QuickJs::eval(
                    enter.token(),
                    "JSON.parse('invalid')",
                    "count.js",
                )?,
            )
        }) {
            Ok(count) => Ok(Some(count)),
            Err(Error::Guest(exception)) if exception.matches("SyntaxError") => {
                eprintln!(
                    "guest rejected the count: {}",
                    exception.message(),
                );

                Ok(None)
            }
            Err(error) => Err(error),
        }
    }
}
```

`GuestException` retains the thrown value and exposes `name`, `message`, optional `stack`, and matching
against recorded constructor names on its prototype chain. JavaScript can throw values that are
not `Error` objects. Conversion, import, borrowing, engine, interruption, timeout, cancellation, and
closed-guest failures retain their own error categories.

Host callables can return application errors that convert into `Error`. For example, with
`thiserror` as an application dependency:

```rust
#[derive(Debug, thiserror::Error)]
#[error("geometry operation failed")]
struct GeometryError;

impl From<GeometryError> for Error {
    fn from(error: GeometryError) -> Self {
        Error::sourced_unexpected(error.to_string(), error)
    }
}
```

### Typed host exceptions

Derive `HostException`, register the exception with the host module, and throw it using the active
entry context:

```rust
use guestjs::prelude::*;

#[derive(HostException)]
#[guestjs(builtin = "TypeError")]
struct InvalidInput;

struct Validation;

#[guestjs::host_module(
    name = "host_validation",
    backend = B,
    exceptions(InvalidInput),
)]
impl Validation {
    #[guestjs(function)]
    fn check(
        #[guestjs(enter)] enter: &Enter<'_, B>,
        value: String,
    ) -> Result<(), Error>
    where
        B: BackendClasses + BackendExceptions,
    {
        if value.is_empty() {
            return Err(InvalidInput::throw(enter, "value must not be empty"));
        }

        Ok(())
    }
}
```

JavaScript receives the registered error class through its normal exception model:

```javascript
import { check, InvalidInput } from "host_validation";

try {
    check("");
} catch (error) {
    if (!(error instanceof InvalidInput)) {
        throw error;
    }

    const message = error.message;
}
```

If JavaScript does not catch the exception, Rust receives it as `Error::Guest`. Match the typed
exception without re-entering the guest:

```rust
match guest
    .host_module("host_validation")?
    .get::<Function<_>, _>("check")?
    .call::<(), _, _>((), ("",))
{
    Ok(()) => {},
    Err(error) if InvalidInput::caught(&error) => {
        eprintln!("invalid input: {error}");
    },
    Err(Error::Guest(exception)) if exception.matches("RangeError") => {
        eprintln!("out of range: {}", exception.message());
    },
    Err(error) => return Err(error),
}
```

`HostException::caught` checks the exception's recorded name hierarchy. Host exceptions expose a
name, a built-in base, and a message. Supported bases are `Error`, `TypeError`, `RangeError`,
`ReferenceError`, and `SyntaxError`.

## License

GuestJS is licensed under the ISC License. See [LICENSE](LICENSE).

## Support and feedback

Report problems and feature requests through the
[repository issue tracker](https://github.com/wizrds/guestjs/issues).
