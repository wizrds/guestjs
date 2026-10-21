use guestjs_core::{enter::Enter, host::native::NativeLibrary};

#[cfg(any(feature = "llrt-buffer", feature = "llrt-fs"))]
use guestjs_core::host::native::{NativeInitializer, NativeModule};

use crate::{
    backend::{QuickJs, library::NativeModule as QuickJsModule},
    error::Capture,
};

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

    pub fn build(self) -> NativeLibrary<QuickJs> {
        self.libraries.into_iter().collect()
    }
}
