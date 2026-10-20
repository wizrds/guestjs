#[allow(unused_extern_crates)]
extern crate self as guestjs_core;

pub mod backend;
pub mod builder;
pub mod bundle;
pub mod catalog;
pub mod driver;
pub mod enter;
pub mod errors;
pub mod extensions;
pub mod guest;
pub mod handle;
pub mod eval;
pub mod host;
pub mod imports;
pub mod marshal;
pub mod policy;
pub mod runtime;

#[cfg(feature = "embedded")]
pub mod embed {
    pub use include_dir::{
        Dir,
        DirEntry,
        File,
    };

    pub mod __include_dir {
        pub use include_dir::{
            Dir,
            DirEntry,
            File,
        };
    }

    pub use include_dir::include_dir as __include_dir_macro;
}
