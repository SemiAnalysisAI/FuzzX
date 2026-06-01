# 033 — atomicrmw fadd float lowers to atom.add.f32, which unconditionally flushes subnormals (FTZ) even in IEEE denormal mode

- **Kind:** miscompile
- **Reachable via:** default llc
- **Component:** NVPTXISelLowering.cpp 7478-7479 (shouldExpandAtomicRMWInIR), pattern at NVPTXIntrinsics.td:2588  (round-5 area `V06-atomicrmw-fp-expand`)
- **Candidate id:** r5_07

## Summary

`atomicrmw fadd float` always lowers to `atom.add.f32`, which hardware-flushes subnormals even in the default IEEE denormal mode (no non-flushing variant chosen)

## Mechanism / root cause

shouldExpandAtomicRMWInIR returns AtomicExpansionKind::None unconditionally for f32 FAdd:

  if (Ty->isFloatTy())
    return AtomicExpansionKind::None;

This selects the native PTX instruction (NVPTXIntrinsics.td:2588: F_ATOMIC_2<F32RT, atomic_load_fadd, "add.f32", ...>), i.e. `atom.<sem>.<scope>.add.f32`. Per the PTX ISA, `atom.add.f32` ALWAYS flushes subnormal inputs AND subnormal results to sign-preserving zero (it has no .ftz/.noftz qualifier; the flush is hardwired). The note for the f16/bf16 variants is `add.noftz.f16`/`add.noftz.bf16` precisely because those CAN avoid flushing, but f32 cannot.

The IR `atomicrmw fadd float` is a full IEEE floating-point add. In the function's default denormal-fp-math mode ("ieee"), subnormals must be preserved. The decision at line 7478 never consults the denormal mode, so even a strict-IEEE function gets the flushing instruction. This is inconsistent with how NVPTX lowers a plain `fadd float`, which emits non-flushing `add.rn.f32` (confirmed below), and inconsistent with x86 which expands atomicrmw fadd to a CAS loop with a real (non-flushing) `addss`. LLVM treats f32-add subnormal flushing as a miscompile in default mode (cf. llvm-project issue #161342, tagged backend:NVPTX/clang:codegen, with the analogous denormal-result-flushed-to-+0 example). f16/bf16 (.noftz) and f64 (no PTX flush) are NOT affected; only f32.

## Trigger

atomicrmw fadd ptr %p, float %v in a function with the default (ieee) f32 denormal mode. Concrete input: *%p = 0x00000001 (smallest positive subnormal, ~1.4e-45) and %v = 0x00000001. IEEE result: store 0x00000002 (~2.8e-45, still subnormal) and return old value 0x00000001. Emitted atom.add.f32 result: both subnormal inputs flush to +0.0, 0.0+0.0=+0.0, so it stores 0x00000000 and returns the old loaded value as +0.0 (0x00000000). Both the stored memory and the returned value differ from IR semantics. (Also triggers when only the RESULT is subnormal, e.g. largest-subnormal + tiny.)

## Reproducer

```
target triple = "nvptx64-nvidia-cuda"
define float @fadd_f32_ieee(ptr %p, float %v) {
  %r = atomicrmw fadd ptr %p, float %v monotonic
  ret float %r
}
```

Command: `llc -mtriple=nvptx64 -mcpu=sm_90 -o - repro.ll`

## Observed (wrong) output

```
ld.param.b64 	%rd1, [fadd_f32_ieee_param_0];
	ld.param.b32 	%r1, [fadd_f32_ieee_param_1];
	atom.relaxed.sys.add.f32 	%r2, [%rd1], %r1;
	st.param.b32 	[func_retval0], %r2;
	ret;

// atom.add.f32 per PTX ISA flushes subnormal inputs AND results to sign-preserving zero (hardwired, no .noftz). For *%p=0x00000001, %v=0x00000001: stores 0x00000000 and returns 0x00000000.
```

## Expected

For the IR atomicrmw fadd float in default (ieee) denormal mode, subnormals must be preserved. With *%p=0x00000001 (smallest positive subnormal) and %v=0x00000001: the new stored value should be 0x00000002 (~2.8e-45, still subnormal) and the returned old value should be 0x00000001. A correct lowering must not use the always-flushing atom.add.f32; e.g. expand to a CAS loop using a non-flushing add (as x86 does with addss+cmpxchg, and as NVPTX's plain fadd uses add.rn.f32), or only emit atom.add.f32 when the function's f32 denormal mode is preserve-sign (FTZ). Plain `fadd float` for comparison emits add.rn.f32; x86 -mtriple=x86_64 expands to addss + lock cmpxchgl.

## Verification

Independent verify + adversarial refute, both `confirmed` (verify confidence 0.9).

> Confirmed the mechanism in source and empirically. NVPTXISelLowering.cpp:7478-7479 returns AtomicExpansionKind::None unconditionally for f32 atomicrmw fadd without consulting the function's denormal mode. This selects NVPTXIntrinsics.td:2588 (F_ATOMIC_2<F32RT, ..., "add.f32">), and llc emits `atom.relaxed.sys.add.f32` for both ieee and preserve-sign denormal-fp-math modes.

The PTX ISA specifies that atom.add.f32 / red.add.f32 "rounds to nearest even and flushes subnormal inputs and results to sign-preserving zero." Unlike the f16/bf16 atomic adds (which use add.noftz.f16/.bf16, lines 2586-2587) and unlike fmin/fmax (which thread an explicit FTZ/NoFTZ flag), the f32 atomic add has NO qualifier to disable flushing — the flush is hardwired in hardware.

The IR `atomicrmw fadd float` is a full IEEE add. In default (ieee) denormal mode, subnormals must be preserved. Two independent confirmations that IEEE (non-flushing) is the required behavior: (1) NVPTX's own plain `fadd float` emits non-flushing add.rn.f32, and the backend has useF32FTZ() (NVPTXISelLowering.cpp:146-148) that respects denormal mode for ordinary FP but is bypassed on the atomic path; (2) x86 expands the same IR to a C
