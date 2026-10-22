use crate::{
    backend::{buffers::BackendBuffers, interrupt::BackendInterrupt, modules::BackendModules},
    builder::{GuestBuilderExt, RuntimeBuilderExt},
    eval::GuestEvalExt,
    fixtures::guest_fixture,
    handle::buffer::TypedArray,
    runtime::Runtime,
};

guest_fixture! {
    pub fn typed_arrays_borrow_their_elements<B>()
    where B: [
        BackendModules,
        BackendInterrupt,
        BackendBuffers,
    ]
    using Runtime::<B>::builder();
    |guest| {
        assert_eq!(
            guest
                .eval::<TypedArray<B, i32>>("new Int32Array([1, -2, 3])")
                .unwrap()
                .borrow_with(<[i32]>::to_vec)
                .unwrap(),
            vec![1, -2, 3],
        );
    }
}

guest_fixture! {
    pub fn typed_array_writes_land_in_place<B>()
    where B: [
        BackendModules,
        BackendInterrupt,
        BackendBuffers,
    ]
    using Runtime::<B>::builder();
    |guest| {
        guest
            .eval::<TypedArray<B, i32>>("globalThis.values = new Int32Array(4); values")
            .unwrap()
            .write(1, &[7, 8])
            .unwrap();

        assert_eq!(
            guest.eval::<Vec<i32>>("Array.from(values)").unwrap(),
            vec![0, 7, 8, 0],
        );
    }
}

#[macro_export]
macro_rules! __guestjs_backend_buffers_tests {
    ($backend:ty) => {
        #[test]
        fn typed_arrays_borrow_their_elements() {
            $crate::fixtures::buffers::typed_arrays_borrow_their_elements::<$backend>();
        }

        #[test]
        fn typed_array_writes_land_in_place() {
            $crate::fixtures::buffers::typed_array_writes_land_in_place::<$backend>();
        }
    };
}

pub use crate::__guestjs_backend_buffers_tests as tests;
