# GuestJS

GuestJS lets a Rust application load and run JavaScript and TypeScript code, call the functions and read the values that code defines, and expose Rust functions, classes, and modules for guest code to call back into. GuestJS runs on a pluggable backend.

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

Applications choose guest source and decide which host and native capabilities each guest receives. GuestJS does not install JavaScript packages or provide complete Node.js compatibility.

## Installation

Add GuestJS and select its `QuickJs` backend:

```toml
[dependencies]
guestjs = { git = "https://github.com/wizrds/guestjs", features = ["quickjs"] }
```

The backend is reexported through guestjs, so ordinary embedding does not require a separate guestjs-quickjs dependency. There are no default features.

| Feature | Behavior |
| --- | --- |
| `quickjs` | Enables the `QuickJs` backend through `guestjs::quickjs`. |
| `llrt-*` | Enables the matching LLRT library on the `QuickJs` backend. See "LLRT libraries". |
| `embedded` | Enables embedded source/data bundles. |
| `serde` | Enables serde-backed conversion and the `ToGuest`/`FromGuest` derives. |
| `bytes` | Enables `Bytes` conversion for guest binary data. |
| `tokio` | Enables Tokio cancellation support and asynchronous filesystem bundle construction. |

Ordinary guest operations are synchronous. An application chooses its own executor when awaiting guest promises or other asynchronous work. The `tokio` feature is not needed for embedded bundles or ordinary guest calls.

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

    guest.exec(
        r#"
function add(left, right) {
    return left + right;
}
"#,
    )?;

    let sum = guest
        .globals()?
        .function("add")?
        .call::<_, f64>((20.0, 22.0))?;

    assert_eq!(sum, 42.0);

    Ok(())
}
```

`Runtime::<QuickJs>::builder()` selects `QuickJs`. The calls in this example are synchronous; GuestJS converts the arguments and result at the boundary. `Function::call` takes positional arguments and calls with JavaScript `undefined` as `this`; `Function::call_this` supplies one.

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

first.exec("name = 'first'")?;

second.exec("name = 'second'")?;

assert_eq!(first.globals()?.get::<String, _>("name")?, "first");

assert_eq!(second.globals()?.get::<String, _>("name")?, "second");
```

Bind host libraries, native libraries, and bundles to a runtime when all its guests should receive them, or to a guest builder when they belong to one guest. Builders can deny imports by name. Owned handles retain the guest needed for later operations. Closing a guest refuses subsequent entries; final ownership release controls when its retained context can be destroyed.

### Execution control

Configure a timeout and cancellation signal on the runtime when an application needs to stop guest execution:

```rust
use std::time::Duration;

use guestjs::prelude::*;
use guestjs::quickjs::{Config, QuickJs};

let cancellation = Cancellation::new();

let runtime = Runtime::<QuickJs>::builder()
    .config(
        Config::default()
            .memory_limit(32 * 1024 * 1024),
    )
    .timeout(Duration::from_millis(50))
    .cancellation(cancellation.clone())
    .build()?;

let guest = runtime
    .guest()
    .build()?;

assert!(
    guest
        .exec("while (true) {}")
        .is_err_and(|error| error.is_timeout()),
);

cancellation.cancel();

assert!(
    guest
        .eval::<f64>("1 + 1")
        .is_err_and(|error| error.is_cancelled()),
);
```

A guest builder inherits the runtime's default policy and can override it with `policy`. Use the selected backend's configuration type with `RuntimeBuilder::config` for engine-specific settings.

## Loading guest code

Use `Guest::guest_module(path, source)` to load a named JavaScript or TypeScript module and work with its exports:

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
    module.object("settings")?.get::<String, _>("prefix")?,
    "hello",
);

assert_eq!(
    module.function("greet")?.call::<_, String>(("Ada",))?,
    "hello Ada",
);
```

This dynamic handle API is useful when the module shape is not known until runtime. Use `ObjectProtocol::call_method` when the called function must receive the original object as `this`.

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

assert_eq!(module.function("run")?.call::<_, f64>(())?, 42.0);
```

`Bundle` paths name files relative to the source tree. JavaScript module names omit their script extensions. TypeScript is transpiled when added to a bundle; no typescript feature is needed. `Bundle` construction does not infer package.json or turn index.js into a parent module name.

Enable `embedded` for `guestjs::bundle!("$CARGO_MANIFEST_DIR/guest")` and `Bundle::from_embedded`. Enable `tokio` for `Bundle::from_dir(path).await` and the builder's `directory(path).await`. Embedded construction operates on in-memory bytes and stays synchronous. The selected directory's basename is not added as a module prefix. The include_dir dependency's stable embedding path does not guarantee incremental rebuild discovery of newly added files.

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

let math = Math::from(
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

Declare the functions and values the host needs. The generated facade converts each result into the descriptor type declared in the macro.

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

let plugin = Plugin::from(
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

A generated class facade wraps an `Instance<B>` and calls JavaScript methods normally. Use `Object<B>` for unrestricted dynamic access, `Instance<B>` for a dynamic instance view, and `Instance<B, C>` when the live object carries a checked host payload `C` that Rust must borrow.

## Guest-side async

Calling a JavaScript `async` function returns a promise. Convert it to `Promise<B, T>` and await that value from an async Rust function to drive guest progress and produce the fulfilled JavaScript return value as a `T`:

```rust
use guestjs::prelude::*;
use guestjs::quickjs::QuickJs;

#[tokio::main]
async fn main() -> Result<(), Error> {
    let guest = Runtime::<QuickJs>::builder()
        .build()?
        .guest()
        .build()?;

    guest.exec(
        r#"
async function double(value) {
    return value * 2;
}
"#,
    )?;

    let doubled = guest
        .globals()?
        .function("double")?
        .call::<_, Promise<_, f64>>((21.0,))?
        .await?;

    assert_eq!(doubled, 42.0);

    Ok(())
}
```

This example uses Tokio as the application's executor, with its `macros` and `rt-multi-thread` features enabled. GuestJS does not select an executor. A `Promise<B, T>` requires a promise result. `Awaitable<T>` accepts any value the way JavaScript `await` does, adopting a thenable and wrapping any other value, and converts to `Promise<B, T>`. Other host and guest operations are synchronous; module evaluation and asynchronous iteration expose their own explicit async operations.

## Plain Rust data

Derive `ToGuest` and `FromGuest` for ordinary serde-compatible Rust structs and enums under the `serde` feature. The derives follow the type's `serde::Serialize` and `serde::de::DeserializeOwned` implementations:

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

The derives follow the type's serde representation. Standard Rust primitives, options, collections, arrays, and tuples also cross the boundary directly. Use a host class when guest code needs to retain Rust object identity rather than receive copied data.

### Union inputs

Mark an enum with `#[guestjs(union)]` when a host API accepts several guest value types. Every variant must contain exactly one unnamed field:

```rust
#[derive(guestjs::FromGuest, guestjs::ToGuest)]
#[guestjs(union)]
enum Identifier {
    Number(f64),
    Text(String),
}
```

`FromGuest` tries variants in declaration order and selects the first conversion that succeeds. A conversion mismatch continues to the next variant, while guest exceptions and other failures return immediately. Mismatch descriptions combine and deduplicate the accepted type names. Each attempt sees the same guest object, so effects of an earlier attempt are not rolled back.

Use `#[guestjs(union, backend = B)]` when the enum already declares `B` as its backend parameter. Without `backend = B`, the derives introduce their own backend parameter. Union derives require the `serde` feature as well.

## Arrays

`Array<B>` retains a live JavaScript array. Cloning the handle preserves the same array; indexed writes through `ObjectProtocol` are visible to JavaScript. `len` reads its logical length, including holes. `to_vec::<T>()` copies indexed values into Rust through the existing element conversions. Use `Vec<T>` when Rust needs a collection copy rather than a live guest array.

```rust
use guestjs::prelude::*;
use guestjs::quickjs::QuickJs;

let guest = Runtime::<QuickJs>::builder()
    .build()?
    .guest()
    .build()?;

guest.exec(
    r#"
globalThis.items = [1, 2, 3];
"#,
)?;

let items = guest
    .globals()?
    .get::<Array<_>, _>("items")?;
let same_items = items.clone();
let copied = items.to_vec::<f64>()?;

items.set(0, 10.0)?;

assert_eq!(same_items.get::<f64, _>(0)?, 10.0);
assert_eq!(copied, [1.0, 2.0, 3.0]);

assert_eq!(items.call_method::<_, _, usize>("push", (4.0,))?, 4);
assert_eq!(items.len()?, 4);
assert!(!items.is_empty()?);

items.delete(1)?;

assert_eq!(items.len()?, 4);
assert_eq!(
    items.to_vec::<Option<f64>>()?,
    [Some(10.0), None, Some(3.0), Some(4.0)],
);

items.set("length", 1)?;

assert_eq!(items.to_vec::<f64>()?, [10.0]);
assert_eq!(guest.eval::<f64>("items[0]")?, 10.0);
```

## Binary data

JavaScript binary data lives in an `ArrayBuffer` and is read and written through views: typed arrays and `DataView`. `ArrayBuffer`, `TypedArray` and `DataView` are live handles to those guest objects. Writes go into guest memory, `to_vec` returns a copy, and `borrow_with` and `borrow_with_mut` lend the elements in place as `&[T]` and `&mut [T]`:

```rust
use guestjs::prelude::*;
use guestjs::quickjs::QuickJs;

let guest = Runtime::<QuickJs>::builder()
    .build()?
    .guest()
    .build()?;

guest.exec(
    r#"
globalThis.pixels = new Uint8Array([1, 2, 3, 4]);
"#,
)?;

let pixels = guest
    .globals()?
    .get::<Uint8Array<_>, _>("pixels")?;

pixels.write(1, &[9, 9])?;

assert_eq!(pixels.to_vec()?, [1, 9, 9, 4]);
assert_eq!(pixels.borrow_with(|values| values[1])?, 9);
assert_eq!(pixels.buffer()?.byte_length()?, 4);
```

Guest code does not run while a borrow is live: calling a function, evaluating code or resolving a promise fails with `Error::Borrow` until the closure returns. `ValueProtocol::borrow_as_with` and `borrow_as_with_mut` lend any `ArrayBuffer`, typed array or `DataView` as `[T]` the same way, and `Args::next_ref` and `Args::next_mut` read `&str`, `&[T]` and `&mut [T]` arguments in a host function. A range of a borrowed slice, such as `&values[offset..offset + length]`, is a view into the same guest memory, so a partial read or write copies nothing.

A slice of a typed array element type becomes a new typed array of that type, so `&[u8]` becomes a `Uint8Array` and `&[f64]` a `Float64Array`. `Vec<T>` and Rust arrays still become JavaScript arrays. Under the `bytes` Cargo feature, `bytes::Bytes` becomes a `Uint8Array` and is read from any `ArrayBuffer`, typed array or `DataView`. Serde byte data under the `serde` feature follows the same rules. A `SharedArrayBuffer` and views over one are not accepted.

`TypedArray::from_values` constructs a fresh typed array from a Rust array, slice, or vector of its element type. The input is copied into guest memory; cloning the resulting handle instead retains the same guest view.

## Host classes

`#[guestjs::host_class]` exposes an ordinary Rust type as a JavaScript class. Mark the constructor, methods, and properties guest code needs:

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

Returning a host-class value to Rust preserves the live guest instance rather than cloning the Rust payload. Use `Instance<B, Vector2>::borrow_with` or `borrow_with_mut` when the host needs direct payload access; ordinary facade calls still use JavaScript dispatch.

The macro also supports mutable methods, constructor-side members, supported well-known symbols, and asynchronous host work. A Rust `&self` or `&mut self` receiver accesses the instance's Rust payload; an injected `this` parameter accesses its JavaScript object.

## Backend-generic host classes

A host class that stores guest values rather than plain Rust data is generic over the backend. Name the backend parameter with `backend` so the macro knows which one it is:

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

A host class does not have to carry the backend in its own type. Naming a parameter the impl does not declare introduces it on the exported members:

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

In both forms `backend` names a type parameter. Whether it is reused from the impl or introduced on the members is decided by the impl. Use `backend(pin = QuickJs)` to pin a class to the `QuickJs` backend. `HostClass` and `ClassDeclaration` remain available for handwritten declarations.

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

Bind one module directly to a runtime or guest builder according to the scope that should receive it. Use `HostLibrary` when that scope needs more than one host module or initializer.

Module state is constructed per realisation and captured into callbacks. `context`, `enter`, and `state` request the `CallContext`, its borrowed `Enter`, and typed `ModuleState` respectively. Handwritten callbacks can use the same `FromContext` API without macros.

`#[guestjs(borrow)]` and `#[guestjs(borrow_mut)]` take an argument by reference instead of copying it. A `&str` borrows a guest string, and `&[T]` or `&mut [T]` borrow the elements of a guest `ArrayBuffer`, typed array or `DataView` in place:

```rust
use guestjs::prelude::*;

struct Buffers;

#[guestjs::host_module(name = "host_buffers")]
impl Buffers {
    #[guestjs(function)]
    fn fill(#[guestjs(borrow_mut)] target: &mut [u8], value: u8) -> Result<(), Error> {
        target.fill(value);

        Ok(())
    }
}
```

### Host iterators and streams

A host function can give the guest a Rust iterator, `Stream` or `Sink` without collecting it first:

| Return | Guest receives |
| --- | --- |
| `Iterable::new(iterator)` | a JavaScript iterator, for `for...of` and spread |
| `AsyncIterable::new(stream)` | a JavaScript async iterator, for `for await...of` |
| `StreamSource::new(stream)` | a `ReadableStream` that pulls from the `Stream` |
| `StreamSink::<_, T>::new(sink)` | a `WritableStream` that reads each chunk as `T` and sends it into the `Sink` |

```rust
use futures::{SinkExt, StreamExt, sink, stream};
use guestjs::prelude::*;

let mut namespace = NamespaceDeclaration::new();

namespace
    .function("numbers", |_, _| Ok(Iterable::new(1..=3)))
    .function("letters", |_, _| {
        Ok(AsyncIterable::new(stream::iter(["a", "b"]).map(Ok::<_, Error>)))
    })
    .function("lines", |_, _| {
        Ok(StreamSource::new(stream::iter(["a", "b"]).map(Ok::<_, Error>)))
    })
    .function("discard", |_, _| {
        Ok(StreamSink::<_, String>::new(sink::drain().sink_map_err(|never| match never {})))
    });
```

```javascript
const counted = [...numbers()];
const collected = [];

for await (const letter of letters()) {
    collected.push(letter);
}

await lines().pipeTo(discard());
```

Streams and async iterators yield `Result` items. An `Err` rejects the guest's pending read and
ends the iteration or errors the stream. The guest pulls items only when it reads, so a slow guest
holds back the Rust stream. Calls from the guest are served in the order they were made.

`StreamSource` and `StreamSink` construct the guest's own `ReadableStream` and `WritableStream`,
so the guest must define them. QuickJS does not define them by default. Enable the
`llrt-stream-web` feature and bind the library:

```toml
[dependencies]
guestjs = { git = "https://github.com/wizrds/guestjs", features = ["quickjs", "llrt-stream-web"] }
```

```rust
use guestjs::prelude::*;
use guestjs::quickjs::{QuickJs, llrt::Llrt};

let runtime = Runtime::<QuickJs>::builder()
    .bind_native(Llrt::builder().stream_web().build())
    .build()?;
```

This also makes `stream/web` and `node:stream/web` importable.

## Native libraries

Native libraries are the backend-specific escape hatch for integrations that cannot be expressed as host declarations. This advanced `QuickJs` example needs a compatible direct rquickjs dependency because it implements that dependency's `ModuleDef`:

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

Use rquickjs 0.14 with full-async enabled to match this backend. Bind a native module directly, or combine native modules and initializers in `NativeLibrary`. Native code is not loaded from source bundles, and binding native modules does not grant complete Node.js compatibility.

### LLRT libraries

The `QuickJs` backend can install modules from [LLRT](https://github.com/awslabs/llrt). Each one
has a `guestjs` feature and a matching `Llrt::builder()` method:

| Feature | Method | Guest gets |
| --- | --- | --- |
| `llrt-abort` | `abort()` | `AbortController`, `AbortSignal` |
| `llrt-buffer` | `buffer()` | `Buffer`, the `buffer` module |
| `llrt-console` | `console()` | `console`, the `console` module |
| `llrt-fetch` | `fetch()` | `fetch`, `Request`, `Response`, `Headers`, `FormData` |
| `llrt-fs` | `fs()` | the `fs` and `fs/promises` modules |
| `llrt-os` | `os()` | the `os` module |
| `llrt-path` | `path()` | the `path` module |
| `llrt-stream-web` | `stream_web()` | `ReadableStream`, `WritableStream`, `TransformStream` and their helpers, the `stream/web` module |
| `llrt-timers` | `timers()` | `setTimeout`, `setInterval`, `setImmediate`, their `clear*` functions, `queueMicrotask`, the `timers` module |
| `llrt-url` | `url()` | `URL`, `URLSearchParams`, the `url` module |

Every module is also importable with a `node:` prefix. `llrt-fetch` also enables `llrt-abort`,
`llrt-buffer`, `llrt-stream-web` and `llrt-url`, and `llrt-fs` also enables `llrt-buffer`.

```toml
[dependencies]
guestjs = { git = "https://github.com/wizrds/guestjs", features = ["quickjs", "llrt-timers", "llrt-url"] }
```

```rust
use guestjs::prelude::*;
use guestjs::quickjs::{QuickJs, llrt::Llrt};

let runtime = Runtime::<QuickJs>::builder()
    .bind_native(
        Llrt::builder()
            .timers()
            .url()
            .build(),
    )
    .build()?;
```

`timers` and `fetch` run on Tokio, so a guest using them must be awaited inside a Tokio runtime.

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

It follows the guest's execution policy. Owned handles retain the guest for later reentry; direct entry does not create a second bound-handle API or allow a borrowed engine value to escape.

## Errors

Fallible guest operations return `Result<T, Error>`. Match a guest exception when the application can recover from it, and propagate other failures:

```rust
use guestjs::prelude::*;
use guestjs::quickjs::QuickJs;

fn read_count(guest: &Guest<QuickJs>) -> Result<Option<f64>, Error> {
    match guest.eval::<f64>("JSON.parse('invalid')") {
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
```

`GuestException` retains the thrown value and exposes `name`, `message`, optional `stack`, and matching against recorded constructor names on its prototype chain. JavaScript can throw values that are not `Error` objects. Conversion, import, borrowing, engine, interruption, timeout, cancellation, and closed-guest failures retain their own error categories.

Host callables can return application errors that convert into `Error`. For example, with `thiserror` as an application dependency:

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

Derive `HostException`, register the exception with the host module, and throw it using the active entry context:

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

If JavaScript does not catch the exception, Rust receives it as `Error::Guest`. Match the typed exception without re-entering the guest:

```rust
match guest
    .host_module("host_validation")?
    .function("check")?
    .call::<_, ()>(("",))
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

`HostException::caught` checks the exception's recorded name hierarchy. Host exceptions expose a name, a built-in base, and a message. Supported bases are `Error`, `TypeError`, `RangeError`, `ReferenceError`, and `SyntaxError`.

## License

GuestJS is licensed under the ISC License. See [LICENSE](LICENSE).

## Support and feedback

Report problems and feature requests through the
[repository issue tracker](https://github.com/wizrds/guestjs/issues).
