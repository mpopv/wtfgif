use std::mem::MaybeUninit;

/// Allocates a vector whose elements may be initialized independently.
pub(crate) fn uninitialized<T>(length: usize) -> Vec<MaybeUninit<T>> {
    let mut values = Vec::with_capacity(length);
    values.resize_with(length, MaybeUninit::uninit);
    values
}

/// Converts a completely initialized `MaybeUninit` vector into an ordinary
/// vector without changing its allocation layout.
///
/// # Safety
///
/// Every element must have been initialized before this function is called.
pub(crate) unsafe fn assume_initialized<T>(values: Vec<MaybeUninit<T>>) -> Vec<T> {
    // `MaybeUninit<T>` has the same size and alignment as `T`, and this keeps
    // the original allocation, length, capacity, and allocator together.
    let mut values = std::mem::ManuallyDrop::new(values);
    unsafe {
        Vec::from_raw_parts(
            values.as_mut_ptr().cast::<T>(),
            values.len(),
            values.capacity(),
        )
    }
}
