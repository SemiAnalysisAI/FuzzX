//! End-to-end smoke test: compile a hand-written PTX `add` kernel with ptxas,
//! launch it on the GPU via the FFI wrapper, and check the result.
//!
//! Also checks that `-O0` and `-O3` produce bit-identical output for this
//! kernel — i.e. the differential oracle works on a known-good program.

use fuzzx_exec::{compile, differential, Cuda};

const ADD_PTX: &str = r#".version 8.8
.target sm_103
.address_size 64

.visible .entry add_kernel(
    .param .u64 in_ptr,
    .param .u64 out_ptr,
    .param .u32 n
)
{
    .reg .pred  %p<2>;
    .reg .b32   %r<5>;
    .reg .b64   %rd<8>;

    ld.param.u64    %rd1, [in_ptr];
    ld.param.u64    %rd2, [out_ptr];
    ld.param.u32    %r1, [n];

    mov.u32         %r2, %tid.x;
    setp.ge.u32     %p1, %r2, %r1;
    @%p1 bra        done;

    cvta.to.global.u64 %rd3, %rd1;
    cvta.to.global.u64 %rd4, %rd2;
    mul.wide.u32    %rd5, %r2, 4;
    add.s64         %rd6, %rd3, %rd5;
    add.s64         %rd7, %rd4, %rd5;

    ld.global.u32   %r3, [%rd6];
    add.u32         %r4, %r3, %r2;
    st.global.u32   [%rd7], %r4;

done:
    ret;
}
"#;

const N: u32 = 32;

fn input_bytes() -> Vec<u8> {
    // in[i] = i + 1, so expected out[i] = (i+1) + i = 2i + 1.
    (0..N).flat_map(|i| (i + 1).to_ne_bytes()).collect()
}

fn run(cubin: &[u8]) -> Vec<u32> {
    let cuda = Cuda::init(0).expect("Cuda::init");
    let bytes = cuda
        .launch(
            cubin,
            "add_kernel",
            (1, 1, 1),
            (N, 1, 1),
            &input_bytes(),
            (N as usize) * 4,
            N,
        )
        .expect("launch");
    bytes
        .chunks_exact(4)
        .map(|c| u32::from_ne_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

#[test]
fn o3_matches_expected() {
    let cubin = compile(ADD_PTX, &["-arch=sm_103", "-O3"]).expect("compile -O3");
    let out = run(&cubin);
    let expected: Vec<u32> = (0..N).map(|i| (i + 1) + i).collect();
    assert_eq!(out, expected);
}

#[test]
fn o0_matches_o3() {
    let cuda = Cuda::init(0).expect("Cuda::init");
    let out = differential(
        &cuda,
        ADD_PTX,
        "-arch=sm_103",
        "add_kernel",
        (1, 1, 1),
        (N, 1, 1),
        &input_bytes(),
        (N as usize) * 4,
        N,
    );
    assert!(out.matches(), "diverged: o0={:?} o3={:?}", out.o0, out.o3);
    assert!(!out.diverged());
}
