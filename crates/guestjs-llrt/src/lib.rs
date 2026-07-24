//! Selective LLRT native-library adapters.

use guestjs_core::native::NativeLibrary;

#[cfg(any(
    feature = "buffer",
    feature = "console",
    feature = "fetch",
    feature = "fs",
))]
use guestjs_core::native::NativeInitializer;

#[cfg(any(feature = "buffer", feature = "console", feature = "fs"))]
use guestjs_core::native::NativeModule;

#[cfg(feature = "fetch")]
use llrt_modules::{
    abort::init as init_abort, fetch::init as init_fetch, stream_web::init as init_stream_web,
    url::init as init_url,
};

#[cfg(any(feature = "buffer", feature = "fetch", feature = "fs"))]
use llrt_modules::buffer::init as init_buffer;

#[cfg(feature = "buffer")]
use llrt_modules::buffer::BufferModule;

#[cfg(feature = "console")]
use llrt_modules::console::{ConsoleModule, init as init_console};

#[cfg(feature = "fs")]
use llrt_modules::fs::{FsModule, FsPromisesModule};

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

    #[cfg(feature = "fetch")]
    fn ensure_url(mut self) -> Self {
        self.library = self
            .library
            .initialize(NativeInitializer::new("llrt:url", init_url));

        self
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
    ),
))]
mod tests {
    #[cfg(feature = "fs")]
    use std::fs;

    use guestjs_core::runtime::Runtime;
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
