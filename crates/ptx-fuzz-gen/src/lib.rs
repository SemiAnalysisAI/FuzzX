//! Turn a fuzzer-provided byte string into a PTX source string.
//!
//! v0 strategy: emit the bytes verbatim (after a printable-ASCII filter)
//! inside a fixed `.entry` scaffold. This is the dumbest possible
//! generator; it relies entirely on coverage feedback to find inputs
//! that drive the parser into interesting states. Once the pipeline is
//! confirmed working we should replace this with a grammar-aware
//! generator (e.g. an `arbitrary::Arbitrary` derived AST).

/// Header emitted before the user-controlled body. `sm_70` is recent
/// enough to be widely supported but old enough that `ptxas` will
/// have lots of code paths for it.
const PTX_PRELUDE: &str = "\
.version 7.0
.target sm_70
.address_size 64

.visible .entry kernel(
) {
";

const PTX_EPILOGUE: &str = "
    ret;
}
";

/// Maximum number of body bytes we'll embed. Caps the cost of a single
/// `ptxas` invocation. Larger inputs are truncated.
const MAX_BODY_BYTES: usize = 4096;

pub fn generate_ptx(data: &[u8]) -> String {
    let body = sanitize_to_ptx_text(data);
    let mut out = String::with_capacity(PTX_PRELUDE.len() + body.len() + PTX_EPILOGUE.len());
    out.push_str(PTX_PRELUDE);
    out.push_str(&body);
    out.push_str(PTX_EPILOGUE);
    out
}

/// Keep only bytes that could plausibly appear in PTX source — printable
/// ASCII plus newlines. Everything else is dropped. The goal is to keep
/// the lexer engaged rather than bailing immediately on a NUL byte.
fn sanitize_to_ptx_text(data: &[u8]) -> String {
    let take = data.len().min(MAX_BODY_BYTES);
    let mut s = String::with_capacity(take);
    for &b in &data[..take] {
        if b == b'\n' || b == b'\t' || (b' '..=b'~').contains(&b) {
            s.push(b as char);
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_produces_minimal_kernel() {
        let s = generate_ptx(&[]);
        assert!(s.contains(".entry kernel"));
        assert!(s.contains("ret;"));
    }

    #[test]
    fn non_ascii_bytes_are_dropped() {
        let s = generate_ptx(&[0x80, b'a', 0xff, b'b']);
        assert!(s.contains("ab"));
        assert!(!s.contains('\u{80}'));
    }

    #[test]
    fn long_input_is_truncated() {
        let big = vec![b'x'; MAX_BODY_BYTES * 4];
        let s = generate_ptx(&big);
        let xs = s.bytes().filter(|&b| b == b'x').count();
        assert_eq!(xs, MAX_BODY_BYTES);
    }
}
