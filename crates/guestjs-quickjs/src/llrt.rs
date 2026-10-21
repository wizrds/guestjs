use guestjs_core::host::native::NativeLibrary;

#[cfg(any(
    feature = "llrt-abort",
    feature = "llrt-buffer",
    feature = "llrt-console",
    feature = "llrt-fetch",
    feature = "llrt-stream-web",
    feature = "llrt-timers",
    feature = "llrt-url",
))]
use guestjs_core::{enter::Enter, host::native::NativeInitializer};

#[cfg(any(
    feature = "llrt-buffer",
    feature = "llrt-console",
    feature = "llrt-fs",
    feature = "llrt-os",
    feature = "llrt-path",
    feature = "llrt-stream-web",
    feature = "llrt-timers",
    feature = "llrt-url",
))]
use guestjs_core::host::native::NativeModule;

use crate::backend::QuickJs;

#[cfg(any(
    feature = "llrt-abort",
    feature = "llrt-buffer",
    feature = "llrt-console",
    feature = "llrt-fetch",
    feature = "llrt-stream-web",
    feature = "llrt-timers",
    feature = "llrt-url",
))]
use crate::error::Capture;

#[cfg(any(
    feature = "llrt-buffer",
    feature = "llrt-console",
    feature = "llrt-fs",
    feature = "llrt-os",
    feature = "llrt-path",
    feature = "llrt-stream-web",
    feature = "llrt-timers",
    feature = "llrt-url",
))]
use crate::backend::library::NativeModule as QuickJsModule;

pub struct Llrt;

impl Llrt {
    pub fn builder() -> LlrtBuilder {
        LlrtBuilder::default()
    }
}

#[derive(Default)]
pub struct LlrtBuilder {
    pub libraries: Vec<NativeLibrary<QuickJs>>,
}

impl LlrtBuilder {
    #[cfg(feature = "llrt-abort")]
    pub fn abort(mut self) -> Self {
        self.libraries.push(
            NativeLibrary::new().extend(NativeInitializer::new(|enter: &Enter<'_, QuickJs>| {
                llrt_modules::abort::init(&enter.token().ctx()).capture(enter.token())
            })),
        );
        self
    }

    #[cfg(feature = "llrt-buffer")]
    pub fn buffer(mut self) -> Self {
        self.libraries.push(
            NativeLibrary::new()
                .extend(NativeInitializer::new(|enter: &Enter<'_, QuickJs>| {
                    llrt_modules::buffer::init(&enter.token().ctx()).capture(enter.token())
                }))
                .extend(
                    NativeModule::new(
                        "buffer",
                        QuickJsModule::new::<llrt_modules::buffer::BufferModule>(),
                    )
                    .alias("node:buffer"),
                ),
        );
        self
    }

    #[cfg(feature = "llrt-console")]
    pub fn console(mut self) -> Self {
        self.libraries.push(
            NativeLibrary::new()
                .extend(NativeInitializer::new(|enter: &Enter<'_, QuickJs>| {
                    llrt_modules::console::init(&enter.token().ctx()).capture(enter.token())
                }))
                .extend(
                    NativeModule::new(
                        "console",
                        QuickJsModule::new::<llrt_modules::console::ConsoleModule>(),
                    )
                    .alias("node:console"),
                ),
        );
        self
    }

    #[cfg(feature = "llrt-fetch")]
    pub fn fetch(mut self) -> Self {
        self.libraries.push(
            NativeLibrary::new().extend(NativeInitializer::new(|enter: &Enter<'_, QuickJs>| {
                llrt_modules::fetch::init(&enter.token().ctx()).capture(enter.token())
            })),
        );
        self
    }

    #[cfg(feature = "llrt-fs")]
    pub fn fs(mut self) -> Self {
        self.libraries.push(
            NativeLibrary::new()
                .extend(
                    NativeModule::new("fs", QuickJsModule::new::<llrt_modules::fs::FsModule>())
                        .alias("node:fs"),
                )
                .extend(
                    NativeModule::new(
                        "fs/promises",
                        QuickJsModule::new::<llrt_modules::fs::FsPromisesModule>(),
                    )
                    .alias("node:fs/promises"),
                ),
        );
        self
    }

    #[cfg(feature = "llrt-os")]
    pub fn os(mut self) -> Self {
        self.libraries.push(
            NativeLibrary::new().extend(
                NativeModule::new("os", QuickJsModule::new::<llrt_modules::os::OsModule>())
                    .alias("node:os"),
            ),
        );
        self
    }

    #[cfg(feature = "llrt-path")]
    pub fn path(mut self) -> Self {
        self.libraries.push(
            NativeLibrary::new().extend(
                NativeModule::new("path", QuickJsModule::new::<llrt_modules::path::PathModule>())
                    .alias("node:path"),
            ),
        );
        self
    }

    #[cfg(feature = "llrt-stream-web")]
    pub fn stream_web(mut self) -> Self {
        self.libraries.push(
            NativeLibrary::new()
                .extend(NativeInitializer::new(|enter: &Enter<'_, QuickJs>| {
                    llrt_modules::stream_web::init(&enter.token().ctx()).capture(enter.token())
                }))
                .extend(
                    NativeModule::new(
                        "stream/web",
                        QuickJsModule::new::<llrt_modules::stream_web::StreamWebModule>(),
                    )
                    .alias("node:stream/web"),
                ),
        );
        self
    }

    #[cfg(feature = "llrt-timers")]
    pub fn timers(mut self) -> Self {
        self.libraries.push(
            NativeLibrary::new()
                .extend(NativeInitializer::new(|enter: &Enter<'_, QuickJs>| {
                    llrt_modules::timers::init(&enter.token().ctx()).capture(enter.token())
                }))
                .extend(
                    NativeModule::new(
                        "timers",
                        QuickJsModule::new::<llrt_modules::timers::TimersModule>(),
                    )
                    .alias("node:timers"),
                ),
        );
        self
    }

    #[cfg(feature = "llrt-url")]
    pub fn url(mut self) -> Self {
        self.libraries.push(
            NativeLibrary::new()
                .extend(NativeInitializer::new(|enter: &Enter<'_, QuickJs>| {
                    llrt_modules::url::init(&enter.token().ctx()).capture(enter.token())
                }))
                .extend(
                    NativeModule::new("url", QuickJsModule::new::<llrt_modules::url::UrlModule>())
                        .alias("node:url"),
                ),
        );
        self
    }

    pub fn build(self) -> NativeLibrary<QuickJs> {
        self.libraries.into_iter().collect()
    }
}
