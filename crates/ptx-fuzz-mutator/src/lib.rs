//! AFL++ custom-mutator shared library.
//!
//! We hook AFL++ at `afl_custom_post_process`: AFL's built-in mutators
//! produce a raw byte buffer; we get a chance to transform it into
//! whatever the target wants to see *before* AFL writes it to the
//! input file (`@@`). Crucially, the *raw* bytes are what get stored
//! in AFL's corpus — so AFL's mutators keep operating on the
//! easy-to-splice byte input, while ptxas only ever sees PTX text.
//!
//! See <https://github.com/AFLplusplus/AFLplusplus/blob/stable/docs/custom_mutators.md>.
//!
//! Symbols exported:
//!  - `afl_custom_init` — required; allocates a per-mutator state object.
//!  - `afl_custom_deinit` — frees that state.
//!  - `afl_custom_post_process` — the core transform: raw bytes → PTX text.

use std::ffi::c_void;

/// Per-mutator state. We own a `Vec<u8>` that backs the buffer we hand
/// back to AFL on each call to `afl_custom_post_process`. AFL holds
/// the returned pointer only until its next mutator call, so reusing
/// the buffer between calls is safe and avoids per-iteration heap
/// churn.
struct State {
    out: Vec<u8>,
}

/// Called once when AFL++ loads the library. The return value is an
/// opaque pointer that AFL passes back to us on every subsequent call.
///
/// The `_afl` argument is a pointer to AFL's `afl_state_t`; we don't
/// touch it (it's a moving target between AFL versions). The `_seed`
/// is AFL's RNG seed — only relevant if we did our own random
/// mutation, which we don't.
///
/// # Safety
/// AFL++ ABI: the returned pointer is opaque to AFL and is only ever
/// passed back to our other hooks. We must return a non-null pointer.
#[no_mangle]
pub extern "C" fn afl_custom_init(_afl: *mut c_void, _seed: u32) -> *mut c_void {
    let s = Box::new(State {
        out: Vec::with_capacity(8 * 1024),
    });
    Box::into_raw(s) as *mut c_void
}

/// Called once on shutdown.
///
/// # Safety
/// `data` must be a pointer previously returned by `afl_custom_init`,
/// or null. AFL++ documents passing the previously returned pointer here.
#[no_mangle]
pub unsafe extern "C" fn afl_custom_deinit(data: *mut c_void) {
    if !data.is_null() {
        drop(Box::from_raw(data as *mut State));
    }
}

/// Called after every mutation, before AFL writes the input to the
/// target. We turn the mutated bytes into a PTX source string.
///
/// AFL's expectation (from the upstream header):
///   ```c
///   size_t afl_custom_post_process(void *data, u8 *buf, size_t buf_size,
///                                  u8 **out_buf);
///   ```
///   The returned size is the new input length; `*out_buf` is set to a
///   buffer valid until the next call to a custom-mutator hook.
///
/// # Safety
/// - `data` must be a pointer returned by `afl_custom_init`.
/// - `buf` must point to `buf_size` initialized bytes (AFL guarantees
///   this).
/// - `out_buf` must be a writable `u8 *`.
#[no_mangle]
pub unsafe extern "C" fn afl_custom_post_process(
    data: *mut c_void,
    buf: *mut u8,
    buf_size: usize,
    out_buf: *mut *const u8,
) -> usize {
    let state = &mut *(data as *mut State);
    let input = std::slice::from_raw_parts(buf, buf_size);
    let ptx = ptx_fuzz_gen::generate_ptx(input);

    state.out.clear();
    state.out.extend_from_slice(ptx.as_bytes());

    *out_buf = state.out.as_ptr();
    state.out.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Drive the C-ABI surface from Rust to confirm it round-trips
    /// without UB under Miri-style scrutiny and produces non-empty PTX
    /// for non-empty input.
    #[test]
    fn round_trip() {
        let data = afl_custom_init(std::ptr::null_mut(), 0);
        assert!(!data.is_null());

        let mut input = b"some random bytes".to_vec();
        let mut out_ptr: *const u8 = std::ptr::null();
        let n =
            unsafe { afl_custom_post_process(data, input.as_mut_ptr(), input.len(), &mut out_ptr) };
        assert!(n > 0);
        assert!(!out_ptr.is_null());
        let produced = unsafe { std::slice::from_raw_parts(out_ptr, n) };
        let s = std::str::from_utf8(produced).unwrap();
        assert!(s.contains(".entry kernel"));

        unsafe { afl_custom_deinit(data) };
    }
}
