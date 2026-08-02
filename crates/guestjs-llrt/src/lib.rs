//! Selective LLRT native-library adapters.

use guestjs_core::native::NativeLibrary;

#[cfg(any(
    feature = "buffer",
    feature = "console",
    feature = "fetch",
    feature = "fs",
    feature = "process-env",
    feature = "timers",
    feature = "url",
))]
use guestjs_core::native::NativeInitializer;

#[cfg(any(
    feature = "buffer",
    feature = "console",
    feature = "fs",
    feature = "os",
    feature = "timers",
    feature = "url",
))]
use guestjs_core::native::NativeModule;

#[cfg(feature = "fetch")]
use llrt_modules::{
    abort::init as init_abort, fetch::init as init_fetch, stream_web::init as init_stream_web,
};

#[cfg(any(feature = "buffer", feature = "fetch", feature = "fs"))]
use llrt_modules::buffer::init as init_buffer;

#[cfg(feature = "buffer")]
use llrt_modules::buffer::BufferModule;

#[cfg(feature = "console")]
use llrt_modules::console::{ConsoleModule, init as init_console};

#[cfg(feature = "fs")]
use llrt_modules::fs::{FsModule, FsPromisesModule};

#[cfg(feature = "os")]
use llrt_modules::os::OsModule;

#[cfg(feature = "timers")]
use llrt_modules::timers::{TimersModule, init as init_timers};

#[cfg(any(feature = "fetch", feature = "url"))]
use llrt_modules::url::init as init_url;

#[cfg(feature = "url")]
use llrt_modules::url::UrlModule;

#[cfg(feature = "process-env")]
use rquickjs::Object as JsObject;

/// An LLRT native-library provider.
pub struct Llrt;

impl Llrt {
    /// Creates an [`LlrtBuilder`](crate::LlrtBuilder).
    pub fn builder() -> LlrtBuilder {
        LlrtBuilder::default()
    }
}

/// A selective LLRT native-library builder.
#[derive(Default)]
pub struct LlrtBuilder {
    library: NativeLibrary,
}

impl LlrtBuilder {
    #[cfg(feature = "fetch")]
    fn ensure_abort(mut self) -> Self {
        self.library = self
            .library
            .initialize(NativeInitializer::new("llrt:abort", init_abort));

        self
    }

    #[cfg(any(feature = "fetch", feature = "fs"))]
    fn ensure_buffer_globals(mut self) -> Self {
        self.library = self
            .library
            .initialize(NativeInitializer::new("llrt:buffer", init_buffer));

        self
    }

    #[cfg(feature = "fetch")]
    fn ensure_stream_web(mut self) -> Self {
        self.library = self
            .library
            .initialize(NativeInitializer::new("llrt:stream-web", init_stream_web));

        self
    }

    #[cfg(any(feature = "fetch", feature = "url"))]
    fn ensure_url(mut self) -> Self {
        self.library = self
            .library
            .initialize(NativeInitializer::new("llrt:url", init_url));

        self
    }

    #[cfg(feature = "process-env")]
    fn initialize_process_env(ctx: &rquickjs::Ctx<'_>) -> rquickjs::Result<()> {
        let globals = ctx.globals();
        let process = match globals.get::<_, Option<JsObject>>("process")? {
            Some(process) => process,
            None => JsObject::new(ctx.clone())?,
        };
        let env = JsObject::new(ctx.clone())?;

        for (name, value) in std::env::vars() {
            env.set(name, value)?;
        }

        process.set("env", env)?;
        globals.set("process", process)
    }

    /// Adds the LLRT buffer capability.
    #[cfg(feature = "buffer")]
    pub fn buffer(mut self) -> Self {
        self.library = self.library.with(
            NativeModule::new("buffer", BufferModule)
                .alias("node:buffer")
                .initialize(NativeInitializer::new("llrt:buffer", init_buffer)),
        );

        self
    }

    /// Adds the LLRT console capability.
    #[cfg(feature = "console")]
    pub fn console(mut self) -> Self {
        self.library = self.library.with(
            NativeModule::new("console", ConsoleModule)
                .alias("node:console")
                .initialize(NativeInitializer::new("llrt:console", init_console)),
        );

        self
    }

    /// Adds the LLRT fetch capability.
    #[cfg(feature = "fetch")]
    pub fn fetch(mut self) -> Self {
        self = self
            .ensure_abort()
            .ensure_stream_web()
            .ensure_buffer_globals()
            .ensure_url();
        self.library = self
            .library
            .initialize(NativeInitializer::new("llrt:fetch", init_fetch));

        self
    }

    /// Adds the LLRT filesystem capability.
    #[cfg(feature = "fs")]
    pub fn fs(mut self) -> Self {
        self = self.ensure_buffer_globals();
        self.library = self
            .library
            .with(NativeModule::new("fs", FsModule).alias("node:fs"))
            .with(NativeModule::new("fs/promises", FsPromisesModule).alias("node:fs/promises"));

        self
    }

    /// Adds the LLRT operating-system capability.
    #[cfg(feature = "os")]
    pub fn os(mut self) -> Self {
        self.library = self
            .library
            .with(NativeModule::new("os", OsModule).alias("node:os"));

        self
    }

    /// Adds the LLRT process environment snapshot.
    #[cfg(feature = "process-env")]
    pub fn process_env(mut self) -> Self {
        self.library = self
            .library
            .initialize(NativeInitializer::new(
                "guestjs:process-env",
                Self::initialize_process_env,
            ));

        self
    }

    /// Adds the LLRT timers capability.
    #[cfg(feature = "timers")]
    pub fn timers(mut self) -> Self {
        self.library = self.library.with(
            NativeModule::new("timers", TimersModule)
                .alias("node:timers")
                .initialize(NativeInitializer::new("llrt:timers", init_timers)),
        );

        self
    }

    /// Adds the LLRT URL capability.
    #[cfg(feature = "url")]
    pub fn url(mut self) -> Self {
        self.library = self.library.with(
            NativeModule::new("url", UrlModule)
                .alias("node:url")
                .initialize(NativeInitializer::new("llrt:url", init_url)),
        );

        self
    }

    /// Builds a [`NativeLibrary`](guestjs_core::native::NativeLibrary).
    pub fn build(self) -> NativeLibrary {
        self.library
    }
}

#[cfg(all(
    test,
    any(
        feature = "buffer",
        feature = "console",
        feature = "fetch",
        feature = "fs",
        feature = "os",
        feature = "process-env",
        feature = "timers",
        feature = "url",
    ),
))]
mod tests {
    #[cfg(feature = "fs")]
    use std::fs;

    #[cfg(feature = "process-env")]
    use guestjs_core::native::{NativeInitializer, NativeLibrary};
    use guestjs_core::runtime::Runtime;
    #[cfg(feature = "process-env")]
    use rquickjs::Object as JsObject;
    #[cfg(feature = "fs")]
    use tempfile::tempdir;

    use super::Llrt;

    #[cfg(feature = "buffer")]
    const BUFFER_MODULE: &str = r#"
        import { Buffer as BufferAlias } from "buffer";
        import { Buffer as NodeBuffer } from "node:buffer";

        export default [
            BufferAlias === NodeBuffer,
            BufferAlias.from("hello").toString(),
            NodeBuffer.alloc(3).length,
        ].join(":");
    "#;

    #[cfg(feature = "console")]
    const CONSOLE_MODULE: &str = r#"
        import * as imported from "node:console";

        export default [
            typeof globalThis.console.log,
            typeof imported.Console,
        ].join(":");
    "#;

    #[cfg(feature = "fetch")]
    const FETCH_MODULE: &str = r#"
        export default [
            typeof fetch,
            typeof Request,
            typeof Response,
            typeof Headers,
            typeof Blob,
            typeof URL,
            typeof ReadableStream,
            typeof WritableStream,
            new Response(
                "local",
                {
                    status: 201,
                },
            ).status,
        ].join(":");
    "#;

    #[cfg(feature = "fs")]
    const FS_MODULE: &str = r#"
        import { readFile } from "node:fs/promises";

        export default await readFile(globalThis.__guestjsTestPath, "utf8");
    "#;

    #[cfg(feature = "os")]
    const OS_MODULE: &str = r#"
        import os from "os";
        import nodeOs from "node:os";

        export default [
            typeof os.platform,
            os.platform() === nodeOs.platform(),
        ].join(":");
    "#;

    #[cfg(feature = "process-env")]
    const PROCESS_ENV_MODULE: &str = r#"
        export default [
            typeof globalThis.process,
            typeof globalThis.process.env,
            Object.keys(globalThis.process.env).length > 0,
            typeof globalThis.process.exit,
            typeof globalThis.process.kill,
        ].join(":");
    "#;

    #[cfg(feature = "process-env")]
    const PROCESS_ENV_COMPOSITION_MODULE: &str = r#"
        export default [
            globalThis.process.marker,
            typeof globalThis.process.env,
        ].join(":");
    "#;

    #[cfg(feature = "timers")]
    const TIMERS_MODULE: &str = r#"
        import timers from "timers";
        import nodeTimers from "node:timers";

        export default await new Promise((resolve) => {
            setTimeout(
                () => resolve([
                    typeof globalThis.setTimeout,
                    typeof timers.setTimeout,
                    typeof nodeTimers.setTimeout,
                ].join(":")),
                0,
            );
        });
    "#;

    #[cfg(feature = "url")]
    const URL_MODULE: &str = r#"
        import url from "url";
        import nodeUrl from "node:url";

        export default [
            typeof globalThis.URL,
            url.URL === nodeUrl.URL,
            new URL("https://example.com/path").hostname,
            new URLSearchParams("value=two").get("value"),
        ].join(":");
    "#;

    #[cfg(all(feature = "fetch", feature = "url"))]
    const URL_FETCH_MODULE: &str = r#"
        import url from "url";

        export default [
            typeof url.URL,
            typeof globalThis.URL,
            typeof globalThis.fetch,
            new Response(
                "local",
                {
                    status: 202,
                },
            ).status,
        ].join(":");
    "#;

    #[cfg(feature = "buffer")]
    #[tokio::test]
    async fn provides_buffer_module_and_alias() {
        assert_eq!(
            Runtime::builder()
                .build()
                .await
                .unwrap()
                .guest()
                .bind_native(Llrt::builder().buffer().build())
                .build()
                .await
                .unwrap()
                .guest_module("buffer-test.js", BUFFER_MODULE)
                .await
                .unwrap()
                .get::<String>("default")
                .await
                .unwrap(),
            "true:hello:3",
        );
    }

    #[cfg(feature = "console")]
    #[tokio::test]
    async fn provides_console_globals_and_module() {
        assert_eq!(
            Runtime::builder()
                .build()
                .await
                .unwrap()
                .guest()
                .bind_native(Llrt::builder().console().build())
                .build()
                .await
                .unwrap()
                .guest_module("console-test.js", CONSOLE_MODULE)
                .await
                .unwrap()
                .get::<String>("default")
                .await
                .unwrap(),
            "function:function",
        );
    }

    #[cfg(feature = "fs")]
    #[tokio::test]
    async fn provides_filesystem_module_without_granting_it_globally() {
        let temp_dir = tempdir().unwrap();
        let path = temp_dir.path().join("fixture.txt");
        let runtime = Runtime::builder()
            .build()
            .await
            .unwrap();
        let privileged = runtime
            .guest()
            .bind_native(Llrt::builder().fs().build())
            .build()
            .await
            .unwrap();

        fs::write(&path, "guestjs llrt").unwrap();
        privileged
            .globals()
            .await
            .unwrap()
            .set("__guestjsTestPath", path.to_string_lossy().into_owned())
            .await
            .unwrap();

        assert_eq!(
            privileged
                .guest_module("fs-test.js", FS_MODULE)
                .await
                .unwrap()
                .get::<String>("default")
                .await
                .unwrap(),
            "guestjs llrt",
        );
        assert!(
            runtime
                .guest()
                .build()
                .await
                .unwrap()
                .guest_module("restricted-fs.js", FS_MODULE)
                .await
                .is_err(),
        );
    }

    #[cfg(feature = "fetch")]
    #[tokio::test]
    async fn provides_fetch_globals_without_network_access() {
        assert_eq!(
            Runtime::builder()
                .build()
                .await
                .unwrap()
                .guest()
                .bind_native(Llrt::builder().fetch().build())
                .build()
                .await
                .unwrap()
                .guest_module("fetch-test.js", FETCH_MODULE)
                .await
                .unwrap()
                .get::<String>("default")
                .await
                .unwrap(),
            "function:function:function:function:function:function:function:function:201",
        );
    }

    #[cfg(feature = "os")]
    #[tokio::test]
    async fn provides_operating_system_module_and_alias() {
        assert_eq!(
            Runtime::builder()
                .build()
                .await
                .unwrap()
                .guest()
                .bind_native(Llrt::builder().os().build())
                .build()
                .await
                .unwrap()
                .guest_module("os-test.js", OS_MODULE)
                .await
                .unwrap()
                .get::<String>("default")
                .await
                .unwrap(),
            "function:true",
        );
    }

    #[cfg(feature = "process-env")]
    #[tokio::test]
    async fn provides_only_process_environment_snapshot() {
        assert_eq!(
            Runtime::builder()
                .build()
                .await
                .unwrap()
                .guest()
                .bind_native(Llrt::builder().process_env().build())
                .build()
                .await
                .unwrap()
                .guest_module("process-env-test.js", PROCESS_ENV_MODULE)
                .await
                .unwrap()
                .get::<String>("default")
                .await
                .unwrap(),
            format!("object:object:{}:undefined:undefined", std::env::vars().next().is_some(),),
        );
    }

    #[cfg(feature = "process-env")]
    #[tokio::test]
    async fn preserves_existing_process_properties() {
        assert_eq!(
            Runtime::builder()
                .build()
                .await
                .unwrap()
                .guest()
                .bind_native(
                    NativeLibrary::new()
                        .initialize(NativeInitializer::new("test:process-marker", |ctx| {
                            let process = JsObject::new(ctx.clone())?;

                            process.set("marker", "preserved")?;
                            ctx.globals().set("process", process)
                        },))
                        .extend(Llrt::builder().process_env().build()),
                )
                .build()
                .await
                .unwrap()
                .guest_module("process-env-composition.js", PROCESS_ENV_COMPOSITION_MODULE)
                .await
                .unwrap()
                .get::<String>("default")
                .await
                .unwrap(),
            "preserved:object",
        );
    }

    #[cfg(feature = "timers")]
    #[tokio::test]
    async fn provides_timer_globals_module_and_alias() {
        assert_eq!(
            Runtime::builder()
                .build()
                .await
                .unwrap()
                .guest()
                .bind_native(Llrt::builder().timers().build())
                .build()
                .await
                .unwrap()
                .guest_module("timers-test.js", TIMERS_MODULE)
                .await
                .unwrap()
                .get::<String>("default")
                .await
                .unwrap(),
            "function:function:function",
        );
    }

    #[cfg(feature = "url")]
    #[tokio::test]
    async fn provides_url_globals_module_and_alias() {
        assert_eq!(
            Runtime::builder()
                .build()
                .await
                .unwrap()
                .guest()
                .bind_native(Llrt::builder().url().build())
                .build()
                .await
                .unwrap()
                .guest_module("url-test.js", URL_MODULE)
                .await
                .unwrap()
                .get::<String>("default")
                .await
                .unwrap(),
            "function:true:example.com:two",
        );
    }

    #[cfg(all(feature = "fetch", feature = "url"))]
    #[tokio::test]
    async fn composes_url_and_fetch_initializers() {
        assert_eq!(
            Runtime::builder()
                .build()
                .await
                .unwrap()
                .guest()
                .bind_native(Llrt::builder().url().fetch().build())
                .build()
                .await
                .unwrap()
                .guest_module("url-fetch-test.js", URL_FETCH_MODULE)
                .await
                .unwrap()
                .get::<String>("default")
                .await
                .unwrap(),
            "function:function:function:202",
        );
    }

    #[cfg(feature = "buffer")]
    #[tokio::test]
    async fn reuses_buffer_module_across_guest_contexts() {
        let runtime = Runtime::builder()
            .bind_native(Llrt::builder().buffer().build())
            .build()
            .await
            .unwrap();

        {
            let first = runtime.guest().build().await.unwrap();
            let second = runtime.guest().build().await.unwrap();

            assert!(
                first
                    .guest_module("first-buffer.js", BUFFER_MODULE)
                    .await
                    .is_ok(),
            );
            assert!(
                second
                    .guest_module("second-buffer.js", BUFFER_MODULE)
                    .await
                    .is_ok(),
            );
        }

        assert!(
            runtime
                .guest()
                .build()
                .await
                .unwrap()
                .guest_module("later-buffer.js", BUFFER_MODULE)
                .await
                .is_ok(),
        );
    }

    #[cfg(feature = "buffer")]
    #[tokio::test]
    async fn supports_global_and_local_libraries() {
        assert!(
            Runtime::builder()
                .bind_native(Llrt::builder().buffer().build())
                .build()
                .await
                .unwrap()
                .guest()
                .build()
                .await
                .unwrap()
                .guest_module("global-buffer.js", BUFFER_MODULE)
                .await
                .is_ok(),
        );

        let runtime = Runtime::builder()
            .build()
            .await
            .unwrap();

        assert!(
            runtime
                .guest()
                .bind_native(Llrt::builder().buffer().build())
                .build()
                .await
                .unwrap()
                .guest_module("local-buffer.js", BUFFER_MODULE)
                .await
                .is_ok(),
        );
        assert!(
            runtime
                .guest()
                .build()
                .await
                .unwrap()
                .guest_module("restricted-buffer.js", BUFFER_MODULE)
                .await
                .is_err(),
        );
    }
}
