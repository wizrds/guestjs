//! Web Streams interop.

mod readable;
mod transform;
mod writable;

pub use readable::{BoundReadableStream, BoundReader, HostReadableStream, ReadableStream, Reader};
pub use transform::{HostTransformStream, TransformStream};
pub use writable::{BoundWritableStream, BoundWriter, HostWritableStream, WritableStream, Writer};

#[cfg(test)]
mod tests {
    use guestjs_core::runtime::Runtime;

    use crate::Llrt;

    #[tokio::test]
    async fn streams_capability_exports_module_and_globals() {
        assert_eq!(
            Runtime::builder()
                .build()
                .await
                .unwrap()
                .guest()
                .bind_native(Llrt::builder().streams().build())
                .build()
                .await
                .unwrap()
                .guest_module(
                    "streams.js",
                    r#"
                        import { ReadableStream as Imported } from "stream/web";

                        export default [
                            typeof globalThis.ReadableStream,
                            typeof globalThis.WritableStream,
                            typeof Imported,
                        ].join(":");
                    "#,
                )
                .await
                .unwrap()
                .get::<String>("default")
                .await
                .unwrap(),
            "function:function:function",
        );
    }
}
