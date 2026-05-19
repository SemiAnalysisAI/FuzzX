//! Generate many programs and verify ptxas accepts every one at both -O0 and
//! -O3. Generator bugs that emit syntactically-invalid PTX, or that produce
//! something one opt level rejects, show up here long before they pollute the
//! divergence inbox of the real fuzzer.

use fuzzx_exec::compile;
use fuzzx_execgen::{bytes_from_seed, generate_from_bytes, TARGET_ARCH};

const PROGRAM_BYTES: usize = 4096;

#[test]
fn ptxas_accepts_warp_size_constant_at_both_opt_levels() {
    let arch_flag = format!("-arch={TARGET_ARCH}");
    let ptx = format!(
        r#".version 8.8
.target {TARGET_ARCH}
.address_size 64

.visible .entry warp_size_smoke(
    .param .u64 out_ptr
)
{{
    .reg .b32 %r<1>;
    .reg .b64 %rd<1>;

    ld.param.u64 %rd0, [out_ptr];
    mov.u32 %r0, WARP_SZ;
    st.global.u32 [%rd0], %r0;
    ret;
}}
"#
    );

    for opt in ["-O0", "-O3"] {
        compile(&ptx, &[arch_flag.as_str(), opt]).unwrap();
    }
}

#[test]
fn ptxas_accepts_membar_at_both_opt_levels() {
    let arch_flag = format!("-arch={TARGET_ARCH}");
    let ptx = format!(
        r#".version 8.8
.target {TARGET_ARCH}
.address_size 64

.visible .entry membar_smoke(
    .param .u64 out_ptr
)
{{
    .reg .b32 %r<1>;
    .reg .b64 %rd<1>;

    ld.param.u64 %rd0, [out_ptr];
    membar.cta ;
    membar.gl  ;
    membar.sys ;
    fence.acq_rel.cta ;
    fence.acq_rel.gpu ;
    fence.acq_rel.sys ;
    fence.sc.cta ;
    fence.sc.gpu ;
    fence.sc.sys ;
    mov.u32 %r0, 1;
    st.global.u32 [%rd0], %r0;
    ret;
}}
"#
    );

    for opt in ["-O0", "-O3"] {
        compile(&ptx, &[arch_flag.as_str(), opt]).unwrap();
    }
}

#[test]
fn ptxas_accepts_prefetch_at_both_opt_levels() {
    let arch_flag = format!("-arch={TARGET_ARCH}");
    let ptx = format!(
        r#".version 8.8
.target {TARGET_ARCH}
.address_size 64

.visible .entry prefetch_smoke(
    .param .u64 in_ptr,
    .param .u64 out_ptr
)
{{
    .reg .b32 %r<1>;
    .reg .b64 %rd<3>;

    ld.param.u64 %rd0, [in_ptr];
    ld.param.u64 %rd1, [out_ptr];
    cvta.to.global.u64 %rd2, %rd0;
    prefetch.global.L1 [%rd2 + 0];
    prefetch.global.L2 [%rd2 + 32];
    prefetch.global.L2::evict_last [%rd2 + 64];
    prefetch.global.L2::evict_normal [%rd2 + 96];
    prefetchu.L1 [%rd0 + 0];
    mov.u32 %r0, 1;
    st.global.u32 [%rd1], %r0;
    ret;
}}
"#
    );

    for opt in ["-O0", "-O3"] {
        compile(&ptx, &[arch_flag.as_str(), opt]).unwrap();
    }
}

#[test]
fn ptxas_accepts_warp_barrier_at_both_opt_levels() {
    let arch_flag = format!("-arch={TARGET_ARCH}");
    let ptx = format!(
        r#".version 8.8
.target {TARGET_ARCH}
.address_size 64

.visible .entry warp_barrier_smoke(
    .param .u64 out_ptr
)
{{
    .reg .b32 %r<1>;
    .reg .b64 %rd<1>;

    ld.param.u64 %rd0, [out_ptr];
    bar.warp.sync 0xffffffff;
    mov.u32 %r0, 1;
    st.global.u32 [%rd0], %r0;
    ret;
}}
"#
    );

    for opt in ["-O0", "-O3"] {
        compile(&ptx, &[arch_flag.as_str(), opt]).unwrap();
    }
}

#[test]
fn ptxas_accepts_warp_collectives_at_both_opt_levels() {
    let arch_flag = format!("-arch={TARGET_ARCH}");
    let ptx = format!(
        r#".version 8.8
.target {TARGET_ARCH}
.address_size 64

.visible .entry warp_collective_smoke(
    .param .u64 out_ptr
)
{{
    .reg .pred %p<5>;
    .reg .b32 %r<4>;
    .reg .b64 %rd<1>;

    ld.param.u64 %rd0, [out_ptr];
    mov.u32 %r0, %tid.x;
    setp.lt.u32 %p0, %r0, 16;
    activemask.b32 %r1;
    add.u32 %r0, %r0, %r1;
    vote.sync.all.pred %p1, %p0, 0xffffffff;
    vote.sync.any.pred %p2, %p0, 0xffffffff;
    vote.sync.uni.pred %p3, %p0, 0xffffffff;
    vote.sync.ballot.b32 %r1, %p0, 0xffffffff;
    add.u32 %r0, %r0, %r1;
    match.sync.any.b32 %r1, %r0, 0xffffffff;
    add.u32 %r0, %r0, %r1;
    match.sync.all.b32 %r1|%p4, %r0, 0xffffffff;
    add.u32 %r0, %r0, %r1;
    shfl.sync.idx.b32 %r1, %r0, 0, 31, 0xffffffff;
    add.u32 %r0, %r0, %r1;
    shfl.sync.up.b32 %r1, %r0, 1, 31, 0xffffffff;
    add.u32 %r0, %r0, %r1;
    shfl.sync.down.b32 %r1, %r0, 1, 31, 0xffffffff;
    add.u32 %r0, %r0, %r1;
    shfl.sync.bfly.b32 %r1, %r0, 1, 31, 0xffffffff;
    add.u32 %r0, %r0, %r1;
    redux.sync.add.u32 %r1, %r0, 0xffffffff;
    add.u32 %r0, %r0, %r1;
    redux.sync.min.u32 %r1, %r0, 0xffffffff;
    add.u32 %r0, %r0, %r1;
    redux.sync.max.u32 %r1, %r0, 0xffffffff;
    add.u32 %r0, %r0, %r1;
    redux.sync.and.b32 %r1, %r0, 0xffffffff;
    add.u32 %r0, %r0, %r1;
    redux.sync.or.b32 %r1, %r0, 0xffffffff;
    add.u32 %r0, %r0, %r1;
    redux.sync.xor.b32 %r1, %r0, 0xffffffff;
    add.u32 %r0, %r0, %r1;
    st.global.u32 [%rd0], %r0;
    ret;
}}
"#
    );

    for opt in ["-O0", "-O3"] {
        compile(&ptx, &[arch_flag.as_str(), opt]).unwrap();
    }
}

#[test]
fn ptxas_accepts_cta_barriers_at_both_opt_levels() {
    let arch_flag = format!("-arch={TARGET_ARCH}");
    let ptx = format!(
        r#".version 8.8
.target {TARGET_ARCH}
.address_size 64

.visible .entry cta_barrier_smoke(
    .param .u64 out_ptr
)
{{
    .reg .b32 %r<1>;
    .reg .b64 %rd<1>;

    ld.param.u64 %rd0, [out_ptr];
    bar.sync 0;
    bar.sync 0, 32;
    barrier.sync 0;
    barrier.sync 0, 32;
    mov.u32 %r0, 1;
    st.global.u32 [%rd0], %r0;
    ret;
}}
"#
    );

    for opt in ["-O0", "-O3"] {
        compile(&ptx, &[arch_flag.as_str(), opt]).unwrap();
    }
}

#[test]
fn ptxas_accepts_cta_barrier_reductions_at_both_opt_levels() {
    let arch_flag = format!("-arch={TARGET_ARCH}");
    let ptx = format!(
        r#".version 8.8
.target {TARGET_ARCH}
.address_size 64

.visible .entry cta_barrier_reduction_smoke(
    .param .u64 out_ptr
)
{{
    .reg .pred %p<3>;
    .reg .b32 %r<4>;
    .reg .b64 %rd<1>;

    ld.param.u64 %rd0, [out_ptr];
    mov.u32 %r0, %tid.x;
    setp.lt.u32 %p0, %r0, 16;
    bar.red.popc.u32 %r1, 0, %p0;
    bar.red.and.pred %p1, 0, %p0;
    bar.red.or.pred %p2, 0, %p0;
    selp.u32 %r2, 1, 0, %p1;
    selp.u32 %r3, 2, 0, %p2;
    add.u32 %r1, %r1, %r2;
    add.u32 %r1, %r1, %r3;
    barrier.red.popc.u32 %r2, 0, %p0;
    barrier.red.and.pred %p1, 0, %p0;
    barrier.red.or.pred %p2, 0, %p0;
    add.u32 %r1, %r1, %r2;
    st.global.u32 [%rd0], %r1;
    ret;
}}
"#
    );

    for opt in ["-O0", "-O3"] {
        compile(&ptx, &[arch_flag.as_str(), opt]).unwrap();
    }
}

#[test]
fn ptxas_accepts_brx_idx_at_both_opt_levels() {
    let arch_flag = format!("-arch={TARGET_ARCH}");
    let ptx = format!(
        r#".version 8.8
.target {TARGET_ARCH}
.address_size 64

.visible .entry brx_idx_smoke(
    .param .u64 out_ptr
)
{{
    .reg .b32 %r<2>;
    .reg .b64 %rd<1>;

    ld.param.u64 %rd0, [out_ptr];
    mov.u32 %r0, %tid.x;
    and.b32 %r1, %r0, 3;
brx_table: .branchtargets brx_0, brx_1, brx_2, brx_3;
    brx.idx %r1, brx_table;
brx_0:
    add.u32 %r0, %r0, 1;
    bra brx_done;
brx_1:
    add.u32 %r0, %r0, 2;
    bra brx_done;
brx_2:
    add.u32 %r0, %r0, 3;
    bra brx_done;
brx_3:
    add.u32 %r0, %r0, 4;
    bra brx_done;
brx_done:
    st.global.u32 [%rd0], %r0;
    ret;
}}
"#
    );

    for opt in ["-O0", "-O3"] {
        compile(&ptx, &[arch_flag.as_str(), opt]).unwrap();
    }
}

#[test]
fn ptxas_accepts_shared_atomic_reduction_at_both_opt_levels() {
    let arch_flag = format!("-arch={TARGET_ARCH}");
    let ptx = format!(
        r#".version 8.8
.target {TARGET_ARCH}
.address_size 64

.visible .entry shared_atomic_smoke(
    .param .u64 out_ptr
)
{{
    .reg .b32 %r<6>;
    .reg .b64 %rd<2>;
    .shared .align 4 .b8 scratch[128];

    ld.param.u64 %rd0, [out_ptr];
    mov.u64 %rd1, scratch;
    mov.u32 %r0, 1;
    mov.u32 %r1, 2;
    st.shared.u32 [%rd1], %r0;
    atom.shared.add.u32 %r2, [%rd1], %r1;
    red.shared.xor.b32 [%rd1], %r1;
    ld.shared.u32 %r3, [%rd1];
    add.u32 %r4, %r2, %r3;
    st.global.u32 [%rd0], %r4;
    ret;
}}
"#
    );

    for opt in ["-O0", "-O3"] {
        compile(&ptx, &[arch_flag.as_str(), opt]).unwrap();
    }
}

#[test]
fn ptxas_accepts_random_programs_at_both_opt_levels() {
    let arch_flag = format!("-arch={TARGET_ARCH}");
    let mut failures: Vec<(u64, String, String)> = Vec::new();

    for seed in 0u64..50 {
        let bytes = bytes_from_seed(seed, PROGRAM_BYTES);
        let ptx = match generate_from_bytes(&bytes) {
            Ok(p) => p,
            Err(e) => {
                failures.push((seed, "generator".into(), format!("{e}")));
                continue;
            }
        };
        for opt in ["-O0", "-O3"] {
            if let Err(e) = compile(&ptx, &[arch_flag.as_str(), opt]) {
                failures.push((seed, opt.into(), format!("{e}\n--- ptx ---\n{ptx}")));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "{} ptxas rejections out of 50 seeds (showing first 3):\n{}",
        failures.len(),
        failures
            .iter()
            .take(3)
            .map(|(seed, opt, msg)| format!("seed={seed} opt={opt}:\n{msg}"))
            .collect::<Vec<_>>()
            .join("\n\n=====\n\n"),
    );
}
