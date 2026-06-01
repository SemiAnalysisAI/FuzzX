# 022 — Scoped atomic min/max intrinsics (int_nvvm_atomic_{max,min}_gen_i_{cta,sys}) always lower to signed PTX (.s32/.s64); unsigned CUDA atomicMax_block/Min on unsigned values is miscompiled

- **Kind:** miscompile
- **Reachable via:** default llc, sm_70+
- **Component:** NVPTXIntrinsics.td 2722-2727 (ATOM2_minmax_impl); 2651-2677 (ATOM2N_impl/ATOM2S_impl); selected vs IntrinsicsNVVM.td:1918-1919  (round-3 area `T24-intrinsics-atom-warp`)
- **Candidate id:** r3_10

## Summary

scoped atomic min/max (`atomicMax_block` etc. on unsigned) always lowers to signed `atom.max.s32`/`.s64`

## Mechanism / root cause

ATOM2_minmax_impl<OpStr> emits TWO instruction patterns for the SAME intrinsic family: `defm _s32 : ATOM2S_impl<OpStr,"i","s32",...>` (-> atom.max.s32) and `defm _u32 : ATOM2S_impl<OpStr,"i","u32",...>` (-> atom.max.u32). Both pass IntTypeStr="i", so ATOM2N_impl builds op = int_nvvm_atomic_max_gen_i_{scope} for BOTH. There is only ONE such intrinsic (IntrinsicsNVVM.td:1918 `defm int_nvvm_atomic_max_gen_i : PTXAtomicWithScope2<llvm_anyint_ty>` — overloaded on int type, carrying NO signedness). ISel therefore always picks the first/signed pattern. Verified: the intrinsic always emits `atom.cta.max.s32`/`.s64` and the .u32/.u64 instructions are dead. This is an end-to-end miscompile, not just an ambiguous-IR nitpick: clang routes BOTH signed and unsigned builtins to the same intrinsic — clang/lib/CodeGen/TargetBuiltins/NVPTX.cpp:618-622 lists __nvvm_atom_cta_max_gen_i (signed) AND __nvvm_atom_cta_max_gen_ui (unsigned) both -> Intrinsic::nvvm_atomic_max_gen_i_cta; clang/test/CodeGen/builtins-nvptx.c:463 (signed) and :466 (unsigned) both CHECK `call i32 @llvm.nvvm.atomic.max.gen.i.cta.i32.p0`. So CUDA `atomicMax_block`/`atomicMin_block`/`atomicMax_system`/`atomicMin_system` on `unsigned`/`unsigned long`/`unsigned long long` compile to a SIGNED max/min. (By contrast the non-scoped atomicrmw path correctly uses distinct atomic_load_max vs atomic_load_umax SDNodes.) Concrete: *p=0x80000000, v=1: unsigned max must yield 0x80000000 (2147483648 > 1) but atom.max.s32 yields 1 (1 > -2147483648). Affects min and max, 32- and 64-bit, both cta and sys scopes.

## Trigger

Call llvm.nvvm.atomic.max.gen.i.cta.i32 (or .min / .sys / .i64) — i.e. CUDA __nvvm_atom_cta_max_gen_ui / atomicMax_block(unsigned*) — on a value whose unsigned and signed orderings differ (high bit set), e.g. existing 0x80000000 vs argument 1.

## Reproducer

```
target triple = "nvptx64-nvidia-cuda"

declare i32 @llvm.nvvm.atomic.max.gen.i.cta.i32(ptr, i32)
declare i64 @llvm.nvvm.atomic.max.gen.i.cta.i64(ptr, i64)
declare i32 @llvm.nvvm.atomic.min.gen.i.cta.i32(ptr, i32)
declare i32 @llvm.nvvm.atomic.max.gen.i.sys.i32(ptr, i32)

define i32 @umax32(ptr %p, i32 %v) {
  %r = call i32 @llvm.nvvm.atomic.max.gen.i.cta.i32(ptr %p, i32 %v)
  ret i32 %r
}

define i64 @umax64(ptr %p, i64 %v) {
  %r = call i64 @llvm.nvvm.atomic.max.gen.i.cta.i64(ptr %p, i64 %v)
  ret i64 %r
}

define i32 @umin32(ptr %p, i32 %v) {
  %r = call i32 @llvm.nvvm.atomic.min.gen.i.cta.i32(ptr %p, i32 %v)
  ret i32 %r
}

define i32 @umax32_sys(ptr %p, i32 %v) {
  %r = call i32 @llvm.nvvm.atomic.max.gen.i.sys.i32(ptr %p, i32 %v)
  ret i32 %r
}
```

Command: `llc -mtriple=nvptx64 -mcpu=sm_70 -mattr=+ptx60 -o - repro.ll`

## Observed (wrong) output

```
umax32:   atom.cta.max.s32 	%r2, [%rd1], %r1;
umax64:   atom.cta.max.s64 	%rd3, [%rd1], %rd2;
umin32:   atom.cta.min.s32 	%r2, [%rd1], %r1;
umax32_sys: atom.sys.max.s32 	%r2, [%rd1], %r1;

(All four scoped min/max intrinsics lower to the SIGNED PTX instruction. For umax32 with *p=0x80000000, v=1, atom.cta.max.s32 computes signed max(-2147483648, 1)=1 and stores 1 into *p.)
```

## Expected

When the value is the result of an UNSIGNED CUDA atomic (e.g. atomicMax_block(unsigned*), builtin __nvvm_atom_cta_max_gen_ui, which clang routes to this same intrinsic), the backend should emit the unsigned PTX form `atom.cta.max.u32` (and .u64 / .min.u32 / .sys.max.u32). For *p=0x80000000, v=1 the unsigned max must keep *p = 0x80000000 (2147483648 > 1). The emitted signed atom.cta.max.s32 instead overwrites *p with 1, which is wrong. The proper fix is to distinguish signed vs unsigned for these scoped min/max intrinsics (e.g. separate intrinsics or a signedness flag) so the .u32/.u64 instruction patterns — currently dead because both share intrinsic key int_nvvm_atomic_max_gen_i_{cta,sys} — become selectable, mirroring the non-scoped atomicrmw path which already uses distinct atomic_load_max vs atomic_load_umax SDNodes.

## Verification

Independent verify + adversarial refute, both `confirmed` (verify confidence 0.9).

> Confirmed end-to-end. Source-level mechanism (NVPTXIntrinsics.td:2722-2727, ATOM2_minmax_impl): both `defm _s32 : ATOM2S_impl<OpStr,"i","s32",...>` and `defm _u32 : ATOM2S_impl<OpStr,"i","u32",...>` pass IntTypeStr="i", so ATOM2N_impl (lines 2651-2662) builds op = int_nvvm_atomic_max_gen_i_{cta,sys} for BOTH. There is exactly one such intrinsic (IntrinsicsNVVM.td:1918, PTXAtomicWithScope2<llvm_anyint_ty>; SCOPED_ATOMIC2_impl at 1892 is overloaded only on integer type, carrying NO signedness). So two TableGen patterns match the same intrinsic — one emitting atom.max.s32, one atom.max.u32 — and ISel deterministically picks the first (signed); the .u32/.u64 patterns are dead.

Empirically verified with the built llc: ll.nvvm.atomic.max.gen.i.cta.i32 -> `atom.cta.max.s32`, .i64 -> `atom.cta.max.s64`, .min.cta.i32 -> `atom.cta.min.s32`, .max.sys.i32 -> `atom.sys.max.s32`. Never unsigned.

End-to-end, not an IR nitpick: clang (TargetBuiltins/NVPTX.cpp:616-622) routes BOTH the signed builtin __nvvm_atom_cta_max_gen_i AND the unsigned __nvvm_atom_cta_max_gen_ui/_ul/_ull to Intrinsic::nvvm_atomic_max_gen_i_cta. clang/test/CodeGen/builtins-nvptx.c:461 (signed) and :466 (unsigned, on (unsigned int*)) both CHECK the identical IR call @llvm.nvvm.atomic.max.gen.i.cta.i32.p0. These builtins back CUDA atomicMax_block/atomicMin_block/atomicMax_system/atomicMin_system on unsigned types. Hence un
