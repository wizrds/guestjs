# GuestJS

GuestJS is a host-to-guest module execution library for JavaScript and TypeScript. It combines
rquickjs and oxc behind a higher-level Rust API so applications can load guest-provided modules and
use their exports like typed Rust libraries.

The library provides:

- isolated guest contexts sharing one runtime;
- JavaScript and TypeScript module evaluation;
- typed guest-module functions and exported values;
- owned handles for values retained across operations;
- scope-bound handles for composing operations inside one guest entry;
- serde-backed conversion for ordinary Rust data;
- macros for defining Rust host classes and host modules;
- runtime limits, timeouts, cancellation, and garbage-collection controls;
- runtime-global and guest-local capability binding;
- an explicit native-module escape hatch for rquickjs integrations; and
- selective LLRT modules and globals for buffers, console, filesystem access, fetch, timers, URL,
  operating-system information, and environment variables.

GuestJS does not provide package resolution, filesystem source loading, or complete Node.js
compatibility. Applications remain responsible for choosing module source and deciding which host
and native capabilities each guest receives.

## Installation

Add GuestJS and an asynchronous executor to your project:

```toml
[dependencies]
guestjs = { git = "https://github.com/wizrds/guestjs" }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

GuestJS has no default features. Enable only the optional behavior the application needs:

```toml
[dependencies.guestjs]
git = "https://github.com/wizrds/guestjs"
features = [
    "typescript",
    "llrt-buffer",
    "llrt-console",
    "llrt-fetch",
    "llrt-fs",
    "llrt-os",
    "llrt-process-env",
    "llrt-streams",
    "llrt-timers",
    "llrt-url",
    "tokio",
]
```

| Feature | Behavior |
| --- | --- |
| `typescript` | Transpiles TypeScript guest modules with oxc. |
| `llrt-buffer` | Provides LLRT `buffer` and `node:buffer` modules. |
| `llrt-console` | Provides LLRT console globals and modules. |
| `llrt-fetch` | Provides LLRT fetch and its required web globals. |
| `llrt-fs` | Provides LLRT `fs`, `fs/promises`, and Node-prefixed aliases. |
| `llrt-os` | Provides LLRT `os` and `node:os` modules. |
| `llrt-process-env` | Provides a host environment snapshot through `process.env`. |
| `llrt-streams` | Provides Web Streams globals, modules, and host interop types. |
| `llrt-timers` | Provides LLRT timer globals plus `timers` and `node:timers` modules. |
| `llrt-url` | Provides LLRT URL globals plus `url` and `node:url` modules. |
| `tokio` | Accepts `tokio_util::sync::CancellationToken` as a cancellation signal. |

## Quick start

Define a typed facade, build a runtime and guest, load the module source, and wrap the resulting
module handle:

```rust
use guestjs::prelude::*;

guestjs::guest_module! {
    pub module Math {
        fn add(
            left: i32,
            right: i32,
        ) -> i32;

        fn double(
            value: i32,
        ) -> Promise<i32>;

        value answer: i32;
    }
}

const MATH_SOURCE: &str = r#"
export function add(left, right) {
    return left + right;
}

export async function double(value) {
    return value * 2;
}

export const answer = 42;
"#;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let guest = Runtime::builder()
        .memory_limit(16 * 1024 * 1024)
        .build()
        .await?
        .guest()
        .build()
        .await?;

    let math = Math::from(
        guest
            .guest_module("math.js", MATH_SOURCE)
            .await?,
    );

    assert_eq!(math.add(20, 22).await?, 42);
    assert_eq!(math.double(21).await?.await?, 42);
    assert_eq!(math.answer().await?, 42);

    guest
        .scope(async move |scope| {
            let math = math.bind(&scope)?;

            assert_eq!(math.add(1, 2)?, 3);
            assert_eq!(math.double(21)?.await?, 42);

            Ok(())
        })
        .await?;

    Ok(())
}
```

The facade does not load source or grant authority. `Guest::guest_module` loads the source into a
specific guest, and `Math::from` gives that module a typed Rust interface.

Function and value declarations name their successful guest value descriptors. Generated methods
add `Result<..., guestjs::Error>` for lookup, invocation, and conversion failures. A declared
`Promise<T>` adds a second result boundary when the promise is awaited.

## Runtimes, guests, and isolation

A `Runtime` owns the JavaScript engine and module registry. Each call to `Runtime::guest` creates a
builder for a fresh isolated context:

```rust
use guestjs::prelude::*;

let runtime = Runtime::builder()
    .memory_limit(32 * 1024 * 1024)
    .build()
    .await?;

let first = runtime
    .guest()
    .build()
    .await?;

let second = runtime
    .guest()
    .build()
    .await?;

first
    .eval::<()>("globalThis.name = 'first'")
    .await?;

second
    .eval::<()>("globalThis.name = 'second'")
    .await?;

assert_eq!(first.eval::<String>("globalThis.name").await?, "first");
assert_eq!(second.eval::<String>("globalThis.name").await?, "second");
```

Host and native libraries bound to `RuntimeBuilder` are available to every guest created by that
runtime. Libraries bound to `GuestBuilder` are available only to that guest. Runtime bindings are
registered before guest bindings, and the last registration for the same module specifier wins.

The two binding levels use the same host-library type:

```rust
use guestjs::prelude::*;

async fn build_shared_runtime(
    library: HostLibrary,
) -> Result<Runtime, Error> {
    Runtime::builder()
        .bind(library)
        .build()
        .await
}

async fn build_isolated_guest(
    runtime: &Runtime,
    library: HostLibrary,
) -> Result<Guest, Error> {
    runtime
        .guest()
        .bind(library)
        .build()
        .await
}
```

Dropping a guest releases its guest-local registry bindings after its owned descendants are also
dropped. Owned handles retain the context required for later re-entry.

### Execution control

Execution controls are configured once on `RuntimeBuilder` and apply to every guest created by the
runtime:

```rust
use std::time::Duration;

use guestjs::prelude::*;

let cancellation = Cancellation::new();
let runtime = Runtime::builder()
    .memory_limit(64 * 1024 * 1024)
    .max_stack_size(512 * 1024)
    .gc_threshold(8 * 1024 * 1024)
    .execution_timeout(Duration::from_millis(50))
    .cancellation(cancellation.clone())
    .gc_after(128)
    .build()
    .await?;
let guest = runtime
    .guest()
    .build()
    .await?;

assert!(matches!(
    guest
        .eval::<()>("while (true) {}")
        .await,
    Err(Error::Timeout),
));

cancellation.cancel();

assert!(matches!(
    guest
        .eval::<i32>("1 + 1")
        .await,
    Err(Error::Cancelled),
));

runtime.run_gc().await;
```

`memory_limit` caps engine allocation, `max_stack_size` caps the engine call stack, and
`gc_threshold` sets the allocation threshold that triggers engine garbage collection. `gc_after`
runs garbage collection after the configured number of guest executions. `Runtime::run_gc`
requests a collection immediately.

`execution_timeout` bounds the time QuickJS spends executing each guest operation.
`RuntimeBuilder::cancellation` accepts any `CancelSignal`. `Cancellation` is a clonable, one-way
signal that can stop an active operation or reject a later operation. The `tokio` feature also lets
`RuntimeBuilder::cancellation` accept
`tokio_util::sync::CancellationToken`.

`RuntimeBuilder::interrupt_handler` installs a custom synchronous interrupt condition. Returning
`true` stops the current execution with `Error::Interrupted`. Policy-driven interrupts are reported
as `Error::Timeout` or `Error::Cancelled`.

## Owned and scope-bound handles

Owned handles such as `Module`, `Function`, `Object`, `Class`, `Instance`, `Promise<T>`, and
`Awaitable<T>` can be retained outside a guest scope. Their operations are asynchronous because
each operation enters the guest context.

`Guest::scope` enters the context once. Operations inside the callback return bound handles tied to
that live scope, and most operations become synchronous:

```rust
use guestjs::prelude::*;

const PIPELINE_SOURCE: &str = r#"
export function makeAdder(amount) {
    return value => value + amount;
}

export function apply(operation, value) {
    return operation(value);
}
"#;

guestjs::guest_module! {
    module Pipeline {
        #[guestjs(name = "makeAdder")]
        fn make_adder(
            amount: i32,
        ) -> Function;

        fn apply(
            operation: Function,
            value: i32,
        ) -> i32;
    }
}

let guest = Runtime::builder()
    .build()
    .await?
    .guest()
    .build()
    .await?;

let pipeline = Pipeline::from(
    guest
        .guest_module("pipeline.js", PIPELINE_SOURCE)
        .await?,
);

assert_eq!(
    pipeline
        .apply(
            pipeline.make_adder(1).await?,
            41,
        )
        .await?,
    42,
);

assert_eq!(
    guest
        .scope(async move |scope| {
            let pipeline = pipeline.bind(&scope)?;

            assert_eq!(
                pipeline.apply(
                    pipeline.make_adder(1)?,
                    41,
                )?,
                42,
            );

            pipeline.make_adder(10)?
                .into_owned()
        })
        .await?
        .call::<_, i32>((32,))
        .await?,
    42,
);
```

The `Function` descriptor in the facade becomes `Function` outside a scope and
`BoundFunction<'js>` inside one. The same projection applies recursively, so `Promise<Function>`
and `Awaitable<Function>` become owned handles outside a scope and bound handles resolving to bound
functions inside one.

Use `into_owned` when a bound handle must outlive the scope callback. Promotion requires a scope
with an owning guest context; detached host callbacks cannot create owned guest handles.

## Loading and accessing guest modules

`Guest::guest_module` loads JavaScript source and returns an owned module. `Scope::guest_module`
loads source inside an existing scope and returns a bound module.

Typed facades are optional. The handle API remains available for dynamic modules:

```rust
let module = guest
    .guest_module(
        "dynamic.js",
        r#"
export const settings = {
    prefix: "hello",
};

export function greet(name) {
    return `${settings.prefix} ${name}`;
}
"#,
    )
    .await?;

assert_eq!(
    module
        .object("settings")
        .await?
        .get::<String>("prefix")
        .await?,
    "hello",
);
assert_eq!(
    module
        .function("greet")
        .await?
        .call::<_, String>(("Ada",))
        .await?,
    "hello Ada",
);
```

The module handle supports typed values, functions, objects, and classes. Operations on returned
handles preserve owned or bound mode.

### Typed guest-module functions and values

`guest_module!` supports functions and exported values in one interface:

```rust
guestjs::guest_module! {
    pub module Service {
        fn transform(
            request: String,
        ) -> String;

        #[guestjs(name = "makeHandler")]
        fn make_handler() -> Function;

        fn delayed_handler() -> Promise<Function>;

        value version: String;
        value settings: Object;

        #[guestjs(name = "ServiceClient")]
        value client: Class;
    }
}
```

The Rust method name defaults to the guest export name. Use `#[guestjs(name = "...")]` when the
names differ. A function declaration may contain up to four parameters, matching the current tuple
conversion contract.

Values are looked up on every invocation. The facade does not silently cache mutable module
exports. Handle descriptors return owned handles outside a scope and bound handles inside one.

## Plain Rust data

Derive `ToGuest` and `FromGuest` for ordinary serde-backed structs and enums. Types using these
derives need a direct serde dependency:

```toml
[dependencies]
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

#[derive(
    Debug,
    PartialEq,
    serde::Serialize,
    serde::Deserialize,
    guestjs::ToGuest,
    guestjs::FromGuest,
)]
#[serde(rename_all = "camelCase")]
enum Status {
    Pending,
    Complete,
}
```

The derives delegate the complete value representation to serde:

- `ToGuest` uses `serde::Serialize`;
- `FromGuest` uses owned serde deserialization;
- serde field and variant attributes define the JavaScript representation;
- the two directions can be derived independently; and
- generic types retain their existing bounds and receive complete-target serde predicates.

A serde-backed `Option::None` is represented as JavaScript `null`. Direct guestjs conversion also
emits null and accepts null or undefined as `None`.

Use `Nullish<T>` when all three JavaScript states matter:

```rust
match value {
    Nullish::Undefined => {}
    Nullish::Null => {}
    Nullish::Some(value) => println!("{value}"),
}
```

`Nullish<T>` is designed for direct conversion and callable or facade types. It is not a supported
field representation inside a serde-derived aggregate.

Host classes are different from plain data. They preserve guest object identity and Rust borrowing
semantics. Use a separate serde data type when host-class state also needs value-style
serialization.

## Host classes

`#[guestjs::host_class]` generates `HostClass` and its conversion implementations from an inherent
Rust implementation. The guest class name defaults to the Rust type name; `name = "..."` overrides
it.

The following class exposes construction, shared and mutable methods, an accessor, iteration, a
well-known symbol, a constant, a static method, a statics hook, and an owned asynchronous method:

```rust
use std::future::Future;

use guestjs::prelude::*;

#[derive(Clone)]
struct Vector2 {
    x: f64,
    y: f64,
}

#[guestjs::host_class(rename_all = "camelCase")]
impl Vector2 {
    #[guestjs(constructor)]
    fn new(
        x: f64,
        y: f64,
    ) -> Result<Self, Error> {
        Ok(
            Self {
                x,
                y,
            },
        )
    }

    #[guestjs(method)]
    fn length(&self) -> Result<f64, Error> {
        Ok(self.x.hypot(self.y))
    }

    #[guestjs(method, name = "moveBy")]
    fn translate(
        &mut self,
        dx: f64,
        dy: f64,
    ) -> Result<(), Error> {
        self.x += dx;
        self.y += dy;

        Ok(())
    }

    #[guestjs(get)]
    fn x(&self) -> Result<f64, Error> {
        Ok(self.x)
    }

    #[guestjs(set, name = "x")]
    fn set_x(&mut self, value: f64) -> Result<(), Error> {
        self.x = value;

        Ok(())
    }

    #[guestjs(iterable)]
    fn coordinates(&self) -> Result<[f64; 2], Error> {
        Ok([self.x, self.y])
    }

    #[guestjs(symbol = "toPrimitive")]
    fn to_primitive(&self) -> Result<String, Error> {
        Ok(format!("Vector2({}, {})", self.x, self.y))
    }

    #[guestjs(static_method)]
    fn origin() -> Result<Self, Error> {
        Ok(
            Self {
                x: 0.0,
                y: 0.0,
            },
        )
    }

    #[guestjs(constant)]
    const DIMENSIONS: usize = 2;

    #[guestjs(statics)]
    fn add_statics(statics: &mut Namespace) {
        statics.constant("coordinateSystem", "cartesian");
    }

    #[guestjs(async_method)]
    fn length_async(
        &self,
    ) -> Result<impl Future<Output = Result<f64, Error>> + 'static, Error> {
        let (x, y) = (self.x, self.y);

        Ok(async move { Ok(x.hypot(y)) })
    }
}
```

Guest code sees the generated class naturally:

```javascript
const vector = new Vector2(3, 4);

vector.moveBy(1, 2);
vector.x = 6;

console.log(vector.length());
console.log([...vector]);
console.log(String(vector));
console.log(Vector2.dimensions);
console.log(Vector2.origin());
console.log(Vector2.coordinateSystem);
console.log(await vector.lengthAsync());
```

### Host-class parameters

Ordinary parameters use their `FromGuestBound` descriptor. Helper attributes select other
conversion roles:

```rust
#[guestjs(method)]
fn distance_to(
    &self,
    #[guestjs(borrow)] other: &Vector2,
) -> Result<f64, Error> {
    Ok((self.x - other.x).hypot(self.y - other.y))
}
```

Available parameter forms include:

```text
#[guestjs(scope)] scope: &Scope<'_>
#[guestjs(borrow)] vector: &Vector2
#[guestjs(borrow_mut)] vector: &mut Vector2
#[guestjs(as = Function)] callback: BoundFunction<'_>
#[guestjs(rest)] values: Vec<f64>
```

`Option<T>` accepts an omitted, undefined, or null argument as `None`. `Nullish<T>` distinguishes
undefined and null.

Callable errors may be any type implementing `Into<guestjs::Error>`. A borrowing Rust `async fn`
class method is not supported because its future retains the class borrow. `async_method` instead
uses a synchronous Rust method that copies or clones the required state before returning an owned
`'static` future.

## Host modules

`#[guestjs::host_module]` generates a `HostModule` from an inherent implementation. A host module
can register generated classes, synchronous and asynchronous functions, default and named values,
nested objects, and an explicit build hook:

```rust
use guestjs::prelude::*;

struct Geometry {
    expose_origin: bool,
}

#[guestjs::host_module(
    name = "@host/geometry",
    classes(Vector2),
    rename_all = "camelCase",
)]
impl Geometry {
    #[guestjs(default)]
    const DEFAULT_SYSTEM: &'static str = "cartesian";

    #[guestjs(constant)]
    const API_VERSION: i32 = 1;

    #[guestjs(function)]
    fn hypot(
        left: f64,
        right: f64,
    ) -> Result<f64, Error> {
        Ok(left.hypot(right))
    }

    #[guestjs(function)]
    async fn delayed_hypot(
        left: f64,
        right: f64,
    ) -> Result<f64, Error> {
        Ok(left.hypot(right))
    }

    #[guestjs(object)]
    fn metadata(metadata: &mut Namespace) {
        metadata.constant("coordinateSystem", "cartesian");
        metadata.property("precision", 2);
    }

    #[guestjs(build)]
    fn configure(
        &self,
        exports: &mut Exports,
    ) {
        if self.expose_origin {
            exports.constant("originAvailable", true);
        }
    }
}
```

Bind one module directly, or use `HostLibrary::with` to collect heterogeneous modules in
registration order:

```rust
async fn build_shared_geometry() -> Result<Runtime, Error> {
    Runtime::builder()
        .bind(
            HostLibrary::new()
                .with(
                    Geometry {
                        expose_origin: true,
                    },
                ),
        )
        .build()
        .await
}

async fn build_isolated_geometry(
    runtime: &Runtime,
) -> Result<Guest, Error> {
    runtime
        .guest()
        .bind(
            Geometry {
                expose_origin: true,
            },
        )
        .build()
        .await
}
```

Guest JavaScript can import the generated exports:

```javascript
import defaultSystem, {
    Vector2,
    apiVersion,
    delayedHypot,
    hypot,
    metadata,
    originAvailable,
} from "@host/geometry";

const vector = new Vector2(3, 4);

console.log(defaultSystem);
console.log(apiVersion);
console.log(metadata.coordinateSystem);
console.log(originAvailable);
console.log(vector.length());
console.log(hypot(5, 12));
console.log(await delayedHypot(5, 12));
```

Registered host modules can also be opened directly through `Guest::host_module` or
`Scope::host_module` when host code needs a handle to their exports.

Root module getters, setters, and accessors are not supported by the current ESM export boundary.
Define writable properties and accessors inside an object hook, where GuestJS installs them on an
ordinary JavaScript object. Stateful or fallible dynamic registration remains available through
the explicit build hook and handwritten `HostModule` implementation.

## Native libraries

Native libraries are the low-level escape hatch for modules written directly against rquickjs.
This boundary intentionally exposes rquickjs types; ordinary GuestJS host modules should use
`HostModule` instead.

A `NativeModule` adapts an rquickjs `ModuleDef`. Aliases route additional specifiers to the same
module, while initializers install context-level state for every guest receiving the library:

```rust
use guestjs::prelude::*;
use rquickjs::{
    Ctx,
    module::{Declarations, Exports as JsExports, ModuleDef},
};

struct EnvironmentModule;

impl ModuleDef for EnvironmentModule {
    fn declare<'js>(declarations: &Declarations<'js>) -> rquickjs::Result<()> {
        declarations.declare("runtime")?;

        Ok(())
    }

    fn evaluate<'js>(
        _ctx: &Ctx<'js>,
        exports: &JsExports<'js>,
    ) -> rquickjs::Result<()> {
        exports.export("runtime", "guestjs")?;

        Ok(())
    }
}

fn environment_library() -> NativeLibrary {
    NativeLibrary::new()
        .with(
            NativeModule::new("environment", EnvironmentModule)
                .alias("node:environment")
                .initialize(
                    NativeInitializer::new("environment:globals", |ctx| {
                        ctx.globals().set("__environmentReady", true)
                    }),
                ),
        )
        .initialize(
            NativeInitializer::new("application:globals", |ctx| {
                ctx.globals().set("__application", "example")
            }),
        )
}
```

Native support requires a compatible direct `rquickjs` dependency because `ModuleDef`, `Ctx`, and
the module declaration types belong to rquickjs:

```toml
[dependencies]
rquickjs = { version = "0.12", features = ["full-async"] }
```

Bind native libraries globally or to one guest:

```rust
async fn build_shared_runtime(
    library: NativeLibrary,
) -> Result<Runtime, Error> {
    Runtime::builder()
        .bind_native(library)
        .build()
        .await
}

async fn build_isolated_guest(
    runtime: &Runtime,
    library: NativeLibrary,
) -> Result<Guest, Error> {
    runtime
        .guest()
        .bind_native(library)
        .build()
        .await
}
```

`NativeLibrary::extend` combines existing libraries while preserving entry order. Conflicting
module routes follow the same last-registration-wins rule as host libraries.

## LLRT modules

GuestJS provides selective adapters for standard-library-style modules from LLRT. Each builder
method is available only when its corresponding GuestJS feature is enabled:

```rust
use guestjs::{llrt::Llrt, prelude::*};

async fn build_shared_llrt_runtime() -> Result<Runtime, Error> {
    Runtime::builder()
        .bind_native(
            Llrt::builder()
                .buffer()
                .console()
                .timers()
                .url()
                .build(),
        )
        .build()
        .await
}

async fn build_isolated_llrt_guest(
    runtime: &Runtime,
) -> Result<Guest, Error> {
    runtime
        .guest()
        .bind_native(
            Llrt::builder()
                .fs()
                .fetch()
                .os()
                .process_env()
                .build(),
        )
        .build()
        .await
}
```

Capabilities are explicit:

- `.buffer()` adds `buffer`, `node:buffer`, and the required buffer globals;
- `.console()` adds console globals plus the `console` and `node:console` modules;
- `.fs()` adds `fs`, `fs/promises`, `node:fs`, and `node:fs/promises`;
- `.fetch()` installs LLRT's fetch initializer together with its abort, stream, buffer, and URL
  prerequisites;
- `.streams()` adds Web Streams globals plus the `stream/web` and `node:stream/web` modules;
- `.timers()` adds timer globals plus the `timers` and `node:timers` modules;
- `.url()` adds `URL` and `URLSearchParams` globals plus the `url` and `node:url` modules;
- `.os()` adds the `os` and `node:os` modules; and
- `.process_env()` snapshots host environment variables under `globalThis.process.env`.

The environment adapter exposes only `process.env`. It does not provide the complete Node.js or
LLRT process API, including process termination, signals, identity mutation, arguments, or
current-directory functions.

Runtime-global LLRT libraries are reusable across guest contexts. Guest-local LLRT libraries do
not grant the same modules to other guests:

```rust
let runtime = Runtime::builder()
    .build()
    .await?;

assert!(
    runtime
        .guest()
        .bind_native(Llrt::builder().fs().build())
        .build()
        .await?
        .guest_module(
            "filesystem.js",
            r#"
import { readFile } from "node:fs/promises";

export { readFile };
"#,
        )
        .await
        .is_ok(),
);

assert!(
    runtime
        .guest()
        .build()
        .await?
        .guest_module(
            "restricted.js",
            r#"
import { readFile } from "node:fs/promises";

export { readFile };
"#,
        )
        .await
        .is_err(),
);
```

The LLRT adapter supplies the selected native modules and globals. It does not add package
resolution, filesystem source resolution, or complete Node.js compatibility.

## Streams

Enable `llrt-streams` to exchange Web Streams between host Rust code and guest modules. The
`llrt-fetch` feature includes this capability because fetch bodies use the same stream machinery.

Applications that name the default byte type or use the `futures::Stream` and `futures::Sink`
interfaces directly should include those crates:

```toml
[dependencies]
bytes = "1"
futures = "0.3"
guestjs = { git = "https://github.com/wizrds/guestjs", features = ["llrt-streams"] }
```

Install the `stream/web` module and the `ReadableStream`, `WritableStream`, and `TransformStream`
globals on a runtime or an individual guest with `.streams()`:

```rust
use guestjs::{llrt::Llrt, prelude::*};

let guest = Runtime::builder()
    .build()
    .await?
    .guest()
    .bind_native(Llrt::builder().streams().build())
    .build()
    .await?;
```

Every stream type is generic over its chunk descriptor and defaults to `bytes::Bytes`. Byte chunks
cross the boundary as JavaScript `Uint8Array` values. Other chunk types can use the same stream
machinery when they implement the marshal traits required by their direction of travel.

### Host readable streams

`HostReadableStream<T>` exposes a `futures::Stream<Item = Result<T, Error>>` as a guest
`ReadableStream`. Each guest pull advances the Rust source once, preserving chunk boundaries and
backpressure. Guest cancellation drops the retained Rust source.

This host module exposes an HTTP-client-style class backed by an in-memory source:

```rust
use std::future::Future;

use bytes::Bytes;
use futures::stream;
use guestjs::{
    llrt::{Llrt, streams::HostReadableStream},
    prelude::*,
};

struct HttpClient;

#[guestjs::host_class]
impl HttpClient {
    #[guestjs(constructor)]
    fn new() -> Result<Self, Error> {
        Ok(Self)
    }

    #[guestjs(async_method)]
    fn body(
        &self,
    ) -> Result<impl Future<Output = Result<HostReadableStream, Error>> + 'static, Error> {
        Ok(async move {
            Ok(
                HostReadableStream::from_stream(stream::iter([
                    Ok::<_, Error>(Bytes::from_static(b"first")),
                    Ok(Bytes::from_static(b"second")),
                ])),
            )
        })
    }
}

struct HttpModule;

#[guestjs::host_module(
    name = "@host/http",
    classes(HttpClient),
)]
impl HttpModule {}

let guest = Runtime::builder()
    .bind(HttpModule)
    .build()
    .await?
    .guest()
    .bind_native(Llrt::builder().streams().build())
    .build()
    .await?;
```

The guest receives a standard readable stream and consumes chunks incrementally:

```javascript
import { HttpClient } from "@host/http";

export async function readBody() {
    const body = await new HttpClient().body();
    const chunks = [];

    for await (const chunk of body) {
        chunks.push(chunk);
    }

    return chunks;
}
```

### Guest readable streams

`ReadableStream<T>` is an owned handle for a readable created by guest code. Use `collect` for all
remaining chunks, acquire a `Reader<T>` for explicit reads, or consume that reader through its
`futures::Stream` implementation:

```rust
use bytes::Bytes;
use guestjs::{llrt::streams::ReadableStream, prelude::*};

assert_eq!(
    guest
        .guest_module(
            "body.js",
            r#"
export function body() {
    return new ReadableStream({
        start(controller) {
            controller.enqueue(new Uint8Array([1, 2]));
            controller.enqueue(new Uint8Array([3]));
            controller.close();
        },
    });
}
"#,
        )
        .await?
        .function("body")
        .await?
        .call::<_, ReadableStream>(())
        .await?
        .collect()
        .await?,
    vec![Bytes::from_static(&[1, 2]), Bytes::from_static(&[3])],
);
```

`ReadableStream::cancel` signals the producer to stop. `pipe_through`, `pipe_to`, and `tee` compose
the owned handle with transform and writable handles without exposing LLRT stream classes.

### Writable streams

`HostWritableStream<T>` wraps a `futures::Sink<T, Error = Error>` so a guest can write into a Rust
destination. Closing the guest stream flushes and drops the host sink, while aborting drops it
without flushing.

The reverse direction uses `WritableStream<T>` and `Writer<T>`:

```rust
use bytes::Bytes;
use guestjs::llrt::streams::WritableStream;

let writer = module
    .function("destination")
    .await?
    .call::<_, WritableStream>(())
    .await?
    .writer()
    .await?;

writer.write(Bytes::from_static(&[1, 2, 3])).await?;
writer.close().await?;
```

`Writer<T>` also implements `futures::Sink<T, Error = Error>`. Owned stream and writer handles can
be retained across calls. Their bound counterparts, `BoundWritableStream<'js, T>` and
`BoundWriter<'js, T>`, operate within one live guest scope.

### Transform streams

`TransformStream<I, O>` wraps a transform created by the guest. Its `writable` and `readable`
methods expose the two sides as typed GuestJS handles. `HostTransformStream<I, O>` performs the
opposite conversion by exposing an asynchronous Rust mapping as a guest transform:

```rust
use bytes::Bytes;
use guestjs::llrt::streams::HostTransformStream;

let transform = HostTransformStream::from_fn(
    |chunk: Bytes| async move {
        Ok(
            vec![Bytes::from(
                chunk
                    .iter()
                    .map(|byte| byte.to_ascii_uppercase())
                    .collect::<Vec<_>>(),
            )],
        )
    },
);
```

Each mapping can produce zero, one, or multiple output chunks. Guest code receives a standard
`TransformStream` and can use it with `readable.pipeThrough(transform)`.

## TypeScript

Enable the `typescript` feature to use the built-in oxc transpiler:

```toml
[dependencies]
guestjs = {
    git = "https://github.com/wizrds/guestjs",
    features = ["typescript"],
}
```

The configured transpiler receives the module name and source before evaluation. The filename
extension selects TypeScript parsing:

```rust
assert_eq!(
    guest
        .guest_module(
            "colors.ts",
            r#"
enum Color {
    Red,
    Green,
    Blue,
}

export function selected(): Color {
    return Color.Blue;
}
"#,
        )
        .await?
        .function("selected")
        .await?
        .call::<_, i32>(())
        .await?,
    2,
);
```

Applications can replace the default with a custom `Transpiler` through
`RuntimeBuilder::transpiler`. Without the feature or a custom transpiler, source is evaluated as
JavaScript.

## Errors and promises

All GuestJS operations return `guestjs::Error`. Its categories distinguish:

- I/O failures;
- serialization failures;
- JavaScript engine failures;
- guest exceptions;
- host/guest conversion failures;
- transpilation failures;
- interrupted executions;
- timed-out executions;
- cancelled executions; and
- unexpected internal or contract failures.

Host callable errors may use an application-specific type as long as it converts into
`guestjs::Error`:

```rust
#[derive(Debug, thiserror::Error)]
#[error("geometry operation failed")]
struct GeometryError;

impl From<GeometryError> for guestjs::Error {
    fn from(error: GeometryError) -> Self {
        guestjs::Error::sourced_unexpected(
            error.to_string(),
            Some(error),
        )
    }
}
```

A function which must return a JavaScript promise uses `Promise<T>` as its successful descriptor.
Use `Awaitable<T>` when the guest may return either `T` directly or a promise resolving to `T`:

```rust
guestjs::guest_module! {
    module Jobs {
        fn start() -> Promise<String>;

        fn status() -> Awaitable<String>;
    }
}

async fn start_job(jobs: &Jobs) -> Result<String, Error> {
    jobs.start().await?.await
}

async fn read_status(jobs: &Jobs) -> Result<String, Error> {
    jobs.status().await?.await
}
```

`Promise<T>` rejects a direct JavaScript value when the handle is created. `Awaitable<T>` accepts a
direct value or promise and normalizes both when awaited. In either example, the first `?` reports
export lookup, invocation, and handle conversion errors. The second reports promise rejection and
conversion of the final value.

## License

GuestJS is licensed under the ISC License. See [LICENSE](LICENSE).

## Support and feedback

Report problems and feature requests through the
[repository issue tracker](https://github.com/wizrds/guestjs/issues).
