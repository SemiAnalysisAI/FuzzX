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
fn ptxas_accepts_cluster_special_regs_at_both_opt_levels() {
    let arch_flag = format!("-arch={TARGET_ARCH}");
    let ptx = format!(
        r#".version 8.8
.target {TARGET_ARCH}
.address_size 64

.visible .entry cluster_special_reg_smoke(
    .param .u64 out_ptr
)
{{
    .reg .pred %p<1>;
    .reg .b32 %r<16>;
    .reg .b64 %rd<1>;

    ld.param.u64 %rd0, [out_ptr];
    mov.u32 %r0, %clusterid.x;
    mov.u32 %r1, %clusterid.y;
    mov.u32 %r2, %clusterid.z;
    mov.u32 %r3, %nclusterid.x;
    mov.u32 %r4, %nclusterid.y;
    mov.u32 %r5, %nclusterid.z;
    mov.u32 %r6, %cluster_ctaid.x;
    mov.u32 %r7, %cluster_ctaid.y;
    mov.u32 %r8, %cluster_ctaid.z;
    mov.u32 %r9, %cluster_nctaid.x;
    mov.u32 %r10, %cluster_nctaid.y;
    mov.u32 %r11, %cluster_nctaid.z;
    mov.u32 %r12, %cluster_ctarank;
    mov.u32 %r13, %cluster_nctarank;
    mov.pred %p0, %is_explicit_cluster;
    selp.u32 %r14, 1, 0, %p0;
    add.u32 %r0, %r0, %r1;
    add.u32 %r0, %r0, %r2;
    add.u32 %r0, %r0, %r3;
    add.u32 %r0, %r0, %r4;
    add.u32 %r0, %r0, %r5;
    add.u32 %r0, %r0, %r6;
    add.u32 %r0, %r0, %r7;
    add.u32 %r0, %r0, %r8;
    add.u32 %r0, %r0, %r9;
    add.u32 %r0, %r0, %r10;
    add.u32 %r0, %r0, %r11;
    add.u32 %r0, %r0, %r12;
    add.u32 %r0, %r0, %r13;
    add.u32 %r0, %r0, %r14;
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
fn ptxas_accepts_pred_logic_at_both_opt_levels() {
    let arch_flag = format!("-arch={TARGET_ARCH}");
    let ptx = format!(
        r#".version 8.8
.target {TARGET_ARCH}
.address_size 64

.visible .entry pred_logic_smoke(
    .param .u64 out_ptr
)
{{
    .reg .pred %p<4>;
    .reg .b32 %r<3>;
    .reg .b64 %rd<1>;

    ld.param.u64 %rd0, [out_ptr];
    mov.u32 %r0, %tid.x;
    setp.lt.u32 %p0, %r0, 16;
    setp.eq.u32 %p1, %r0, 0;
    and.pred %p2, %p0, %p1;
    or.pred %p3, %p0, %p1;
    xor.pred %p1, %p2, %p3;
    not.pred %p2, %p1;
    selp.u32 %r1, 1, 0, %p1;
    selp.u32 %r2, 2, 0, %p2;
    add.u32 %r0, %r0, %r1;
    add.u32 %r0, %r0, %r2;
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
fn ptxas_accepts_half_precision_at_both_opt_levels() {
    let arch_flag = format!("-arch={TARGET_ARCH}");
    let ptx = format!(
        r#".version 8.8
.target {TARGET_ARCH}
.address_size 64

.visible .entry half_precision_smoke(
    .param .u64 out_ptr
)
{{
    .reg .pred %p<2>;
    .reg .b16 %h<4>;
    .reg .b32 %r<6>;
    .reg .b64 %rd<1>;
    .reg .f32 %f<2>;
    .reg .f64 %fd<1>;

    ld.param.u64 %rd0, [out_ptr];
    mov.b16 %h0, 0x3c00;
    mov.b16 %h1, 0x4000;
    add.rn.f16 %h2, %h0, %h1;
    sub.rn.f16 %h2, %h2, %h0;
    mul.rn.f16 %h2, %h2, %h1;
    fma.rn.f16 %h2, %h0, %h1, %h2;
    min.f16 %h2, %h2, %h1;
    max.f16 %h2, %h2, %h0;
    abs.f16 %h2, %h2;
    neg.f16 %h3, %h2;
    set.lt.u32.f16 %r0, %h0, %h1;
    set.ge.u32.f16 %r1, %h1, %h0;
    setp.lt.f16 %p0, %h0, %h1;
    setp.ge.f16 %p1, %h1, %h0;
    selp.u32 %r2, 1, 0, %p0;
    add.u32 %r0, %r0, %r2;
    selp.u32 %r2, 2, 0, %p1;
    add.u32 %r0, %r0, %r1;
    add.u32 %r0, %r0, %r2;
    selp.b16 %h2, %h0, %h1, %p0;
    selp.b16 %h3, %h1, %h0, %p1;
    cvt.u32.u16 %r2, %h3;
    add.u32 %r0, %r0, %r2;
    cvt.u32.u16 %r2, %h2;
    add.u32 %r0, %r0, %r2;
    cvt.f32.f16 %f0, %h0;
    cvt.f32.f16 %f1, %h1;
    cvt.rn.f16.f32 %h2, %f0;
    cvt.f64.f16 %fd0, %h2;
    cvt.rn.f16.f64 %h3, %fd0;
    cvt.rzi.u32.f16 %r2, %h3;
    add.u32 %r0, %r0, %r2;
    cvt.rzi.s32.f16 %r2, %h3;
    add.u32 %r0, %r0, %r2;
    mov.u32 %r2, 3;
    cvt.rn.f16.u32 %h2, %r2;
    cvt.rn.f16.s32 %h3, %r2;
    cvt.u32.u16 %r2, %h2;
    add.u32 %r0, %r0, %r2;
    cvt.u32.u16 %r2, %h3;
    add.u32 %r0, %r0, %r2;
    cvt.rn.f16x2.f32 %r2, %f0, %f1;
    add.u32 %r0, %r0, %r2;
    mov.b32 %r3, 0x3c004000;
    mov.b32 %r4, 0x40003c00;
    add.rn.f16x2 %r5, %r3, %r4;
    sub.rn.f16x2 %r5, %r5, %r4;
    mul.rn.f16x2 %r5, %r5, %r4;
    fma.rn.f16x2 %r5, %r5, %r4, %r4;
    min.f16x2 %r5, %r5, %r4;
    max.f16x2 %r5, %r5, %r4;
    abs.f16x2 %r5, %r5;
    neg.f16x2 %r5, %r5;
    add.u32 %r0, %r0, %r5;
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
fn ptxas_accepts_cvt_pack_at_both_opt_levels() {
    let arch_flag = format!("-arch={TARGET_ARCH}");
    let ptx = format!(
        r#".version 8.8
.target {TARGET_ARCH}
.address_size 64

.visible .entry cvt_pack_smoke(
    .param .u64 out_ptr
)
{{
    .reg .b32 %r<8>;
    .reg .b64 %rd<1>;

    ld.param.u64 %rd0, [out_ptr];
    mov.s32 %r0, -40000;
    mov.s32 %r1, 40000;
    cvt.pack.sat.s16.s32 %r2, %r0, %r1;
    cvt.pack.sat.u16.s32 %r3, %r0, %r1;
    mov.u32 %r4, 0x88776655;
    cvt.pack.sat.u8.s32.b32 %r5, %r0, %r1, %r4;
    cvt.pack.sat.s8.s32.b32 %r5, %r0, %r1, %r5;
    cvt.pack.sat.u4.s32.b32 %r5, %r0, %r1, %r5;
    cvt.pack.sat.s4.s32.b32 %r5, %r0, %r1, %r5;
    cvt.pack.sat.u2.s32.b32 %r5, %r0, %r1, %r5;
    cvt.pack.sat.s2.s32.b32 %r5, %r0, %r1, %r5;
    add.u32 %r2, %r2, %r3;
    add.u32 %r2, %r2, %r5;
    st.global.u32 [%rd0], %r2;
    ret;
}}
"#
    );

    for opt in ["-O0", "-O3"] {
        compile(&ptx, &[arch_flag.as_str(), opt]).unwrap();
    }
}

#[test]
fn ptxas_accepts_bf16_tf32_conversion_at_both_opt_levels() {
    let arch_flag = format!("-arch={TARGET_ARCH}");
    let ptx = format!(
        r#".version 8.8
.target {TARGET_ARCH}
.address_size 64

.visible .entry bf16_tf32_smoke(
    .param .u64 out_ptr
)
{{
    .reg .b16 %h<8>;
    .reg .b32 %r<8>;
    .reg .b64 %rd<1>;
    .reg .f32 %f<8>;

    ld.param.u64 %rd0, [out_ptr];
    mov.b32 %r0, 0x3f800000;
    mov.b32 %r1, 0x40000000;
    mov.b32 %f0, %r0;
    mov.b32 %f1, %r1;
    cvt.rn.bf16.f32 %h0, %f0;
    cvt.rz.bf16.f32 %h1, %f1;
    cvt.rn.relu.bf16.f32 %h2, %f0;
    cvt.f32.bf16 %f2, %h0;
    mov.b32 %r2, %f2;
    cvt.f32.bf16 %f3, %h1;
    mov.b32 %r3, %f3;
    cvt.rn.bf16x2.f32 %r4, %f0, %f1;
    cvt.rz.bf16x2.f32 %r5, %f0, %f1;
    cvt.rn.relu.bf16x2.f32 %r6, %f0, %f1;
    cvt.rna.tf32.f32 %r7, %f0;
    add.u32 %r2, %r2, %r3;
    add.u32 %r2, %r2, %r4;
    add.u32 %r2, %r2, %r5;
    add.u32 %r2, %r2, %r6;
    add.u32 %r2, %r2, %r7;
    cvt.rn.tf32.f32 %r7, %f1;
    add.u32 %r2, %r2, %r7;
    cvt.rz.relu.tf32.f32 %r7, %f0;
    add.u32 %r2, %r2, %r7;
    cvt.u32.u16 %r7, %h0;
    add.u32 %r2, %r2, %r7;
    cvt.u32.u16 %r7, %h1;
    add.u32 %r2, %r2, %r7;
    st.global.u32 [%rd0], %r2;
    ret;
}}
"#
    );

    for opt in ["-O0", "-O3"] {
        compile(&ptx, &[arch_flag.as_str(), opt]).unwrap();
    }
}

#[test]
fn ptxas_accepts_helper_call_at_both_opt_levels() {
    let arch_flag = format!("-arch={TARGET_ARCH}");
    let ptx = format!(
        r#".version 8.8
.target {TARGET_ARCH}
.address_size 64

.func (.reg .b32 ret0) fuzzx_helper(.reg .b32 a, .reg .b32 b)
{{
    add.u32 ret0, a, b;
    ret;
}}

.visible .entry helper_call_smoke(
    .param .u64 out_ptr
)
{{
    .reg .b32 %r<4>;
    .reg .b64 %rd<1>;

    ld.param.u64 %rd0, [out_ptr];
    mov.u32 %r0, %tid.x;
    mov.u32 %r1, 7;
    call.uni (%r2), fuzzx_helper, (%r0, %r1);
    add.u32 %r3, %r2, %r0;
    st.global.u32 [%rd0], %r3;
    ret;
}}
"#
    );

    for opt in ["-O0", "-O3"] {
        compile(&ptx, &[arch_flag.as_str(), opt]).unwrap();
    }
}

#[test]
fn ptxas_accepts_rich_helper_calls_at_both_opt_levels() {
    let arch_flag = format!("-arch={TARGET_ARCH}");
    let ptx = format!(
        r#".version 8.8
.target {TARGET_ARCH}
.address_size 64

.func (.reg .b32 ret0, .reg .b32 ret1) fuzzx_helper_pair(.reg .b32 a, .reg .b32 b, .reg .b32 c)
{{
    add.u32 ret0, a, b;
    xor.b32 ret1, ret0, c;
    ret;
}}

.func (.reg .b32 ret0) fuzzx_helper_chain(.reg .b32 a, .reg .b32 b, .reg .b32 c, .reg .b32 d)
{{
    xor.b32 ret0, a, b;
    add.u32 ret0, ret0, c;
    xor.b32 ret0, ret0, d;
    ret;
}}

.func (.param .b32 ret0) fuzzx_param_helper(.param .b32 a, .param .b32 b)
{{
    .reg .b32 %phr<3>;
    ld.param.u32 %phr0, [a];
    ld.param.u32 %phr1, [b];
    add.u32 %phr2, %phr0, %phr1;
    st.param.b32 [ret0], %phr2;
    ret;
}}

.visible .entry rich_helper_call_smoke(
    .param .u64 out_ptr
)
{{
    .reg .b32 %r<8>;
    .reg .b64 %rd<1>;
    .param .b32 fuzzx_param_ret;
    .param .b32 fuzzx_param_a;
    .param .b32 fuzzx_param_b;

    ld.param.u64 %rd0, [out_ptr];
    mov.u32 %r0, %tid.x;
    mov.u32 %r1, 7;
    mov.u32 %r2, 11;
    call.uni (%r3, %r4), fuzzx_helper_pair, (%r0, %r1, %r2);
    call (%r5), fuzzx_helper_chain, (%r3, %r4, %r1, %r2);
    st.param.b32 [fuzzx_param_a], %r5;
    st.param.b32 [fuzzx_param_b], %r0;
    call.uni (fuzzx_param_ret), fuzzx_param_helper, (fuzzx_param_a, fuzzx_param_b);
    ld.param.u32 %r7, [fuzzx_param_ret];
    add.u32 %r6, %r3, %r4;
    add.u32 %r6, %r6, %r5;
    add.u32 %r6, %r6, %r7;
    st.global.u32 [%rd0], %r6;
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
fn ptxas_accepts_cache_policy_helpers_at_both_opt_levels() {
    let arch_flag = format!("-arch={TARGET_ARCH}");
    let ptx = format!(
        r#".version 8.8
.target {TARGET_ARCH}
.address_size 64

.visible .entry cache_policy_smoke(
    .param .u64 in_ptr,
    .param .u64 out_ptr
)
{{
    .reg .b32 %r<4>;
    .reg .b64 %rd<4>;

    ld.param.u64 %rd0, [in_ptr];
    ld.param.u64 %rd1, [out_ptr];
    cvta.to.global.u64 %rd0, %rd0;
    cvta.to.global.u64 %rd1, %rd1;
    createpolicy.fractional.L2::evict_last.L2::evict_unchanged.b64 %rd2, 0.5;
    applypriority.global.L2::evict_normal [%rd0], 128;
    ld.global.L2::cache_hint.u32 %r0, [%rd0], %rd2;
    st.global.L2::cache_hint.u32 [%rd1], %r0, %rd2;
    ld.global.L2::cache_hint.u32 %r1, [%rd1], %rd2;
    add.u32 %r0, %r0, %r1;
    ld.global.nc.L2::cache_hint.u32 %r1, [%rd0], %rd2;
    add.u32 %r0, %r0, %r1;
    st.global.u32 [%rd1 + 4], %r0;
    mov.u32 %r1, 1;
    atom.global.add.L2::cache_hint.u32 %r1, [%rd1 + 4], %r1, %rd2;
    add.u32 %r0, %r0, %r1;
    st.global.u32 [%rd1 + 8], %r0;
    mov.u32 %r1, 1;
    red.global.add.L2::cache_hint.u32 [%rd1 + 8], %r1, %rd2;
    ld.global.u32 %r1, [%rd1 + 8];
    add.u32 %r0, %r0, %r1;
    createpolicy.range.global.L2::evict_last.L2::evict_first.b64 %rd2, [%rd0], 64, 128;
    ld.global.L2::cache_hint.u32 %r1, [%rd0], %rd2;
    add.u32 %r0, %r0, %r1;
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
    elect.sync %r1|%p4, 0xffffffff;
    add.u32 %r0, %r0, %r1;
    selp.u32 %r1, 16, 0, %p4;
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
