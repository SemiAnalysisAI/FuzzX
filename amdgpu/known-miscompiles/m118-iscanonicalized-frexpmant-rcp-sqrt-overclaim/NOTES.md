# m118: `SITargetLowering::isCanonicalized` over-promises ~12 target-ISD nodes and intrinsics as "always canonical", letting sNaN escape `llvm.canonicalize`

*Discovery method: code inspection.*  Sibling shape to m086 (target
hook lies about an instruction whose semantics it never checks).

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/SIISelLowering.cpp:15539-15560`
and `15666-15689` blanket-list these as canonical with no input check:

* SDNodes (15541-15559): `AMDGPUISD::RCP`, `RSQ`, `RSQ_CLAMP`,
  `RCP_LEGACY`, `RCP_IFLAG`, `LOG`, `EXP`, `FRACT`, `DIV_SCALE`,
  `DIV_FMAS`, `DIV_FIXUP`, `SIN_HW`, `COS_HW`, `CVT_PKRTZ_F16_F32`.
* Intrinsics (15670-15683): `amdgcn.cvt_pkrtz`, `amdgcn.cubeid`,
  `amdgcn.frexp_mant`, `amdgcn.fdot2`, `amdgcn.rcp`, `amdgcn.rsq`,
  `amdgcn.rsq_clamp`, `amdgcn.rcp_legacy`, `amdgcn.rsq_legacy`,
  `amdgcn.trig_preop`, `amdgcn.log`, `amdgcn.exp2`, `amdgcn.sqrt`.

The strongest offender: `Intrinsic::amdgcn_frexp_mant`.
`v_frexp_mant_f32` is documented (LLVM intrinsic semantics + AMD ISA) to
return its input unchanged for NaN / ±0 / ±Inf -- sNaN in, sNaN out,
even at `IEEE_MODE=1`.  Hardware only quiets the leading mantissa bit
*on output of an arithmetic op*, not on a frexp_mant identity pass.

`v_rcp_f32(sNaN)`, `v_rsq_f32(sNaN)`, `v_sqrt_f32(sNaN)`,
`v_exp_f32(sNaN)`, `v_log_f32(sNaN)` similarly propagate the NaN
payload (HW quiet bit may be added but the payload sign and bits
beyond the leading mantissa bit are preserved -- not the canonical
qNaN pattern).

The DAGCombiner consumes `isCanonicalized` via the
`fcanonicalize_canonicalized` PatFrag (`SIInstrInfo.td:1013-1018`),
which becomes `COPY` in `SIInstructions.td:3637`.  So a `canonicalize`
wrapping any of these listed nodes is **completely elided**.  The
sNaN escapes to user code.

Smoking-gun codebase self-contradiction:
`llvm/test/CodeGen/AMDGPU/known-never-snan.ll:566-577` -- a test
literally named `v_test_NOT_known_frexp_mant_input_fmed3_r_i_i_f32` --
asserts that `frexp_mant`'s output is *not* known-never-sNaN.  Yet
`isCanonicalized` on line 15672 returns `true`.

## Reproducer

`reduced.ll`:

```llvm
define amdgpu_kernel void @canon_frexp_mant(ptr addrspace(1) %out, float %x) {
  %r = call float @llvm.amdgcn.frexp.mant.f32(float %x)
  %c = call float @llvm.canonicalize.f32(float %r)
  store float %c, ptr addrspace(1) %out
  ret void
}
```

`llc -mcpu=gfx950 -O2`:

```asm
canon_frexp_mant:
        v_frexp_mant_f32_e32 v1, s2
        global_store_dword v0, v1, s[0:1]
        s_endpgm
```

The `llvm.canonicalize` is gone.  Compare baseline
`canon_baseline(float %x)`:

```asm
canon_baseline:
        v_max_f32_e64 v1, s0, s0    ; v_max(x,x) canonicalizes
        global_store_dword v0, v1, s[0:1]
```

So the elision is observable.  Verified for `rcp`, `rsq`, `sqrt`,
`frexp_mant`, `exp2`, `log` -- all behave the same.

## Suggested fix

Replace each of these arms with the recursive
`return isCanonicalized(DAG, Op.getOperand(1), MaxDepth - 1);` pattern
(operand input must already be canonical), or drop them from the
"always canonical" list and rely on the fallback at line 15696-15697
(`denormalsEnabledForType && (NoNaNs || isKnownNeverSNaN)`).

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD (`build/llvm-fuzzer`) | Reproduces (elision verified for all listed intrinsics). |
| ROCm 7.1.1 | Same isCanonicalized table. |

## Why the fuzzer hasn't caught it

* FP emitter rarely generates a bitcast of sNaN bit-pattern (e.g.
  `0x7FA00000`) feeding one of these intrinsics with a
  `llvm.canonicalize` user in the same trace.
* The O0-vs-O2 oracle is blind: the elision is an ISel pattern
  (`fcanonicalize_canonicalized` -> COPY), not a combine pass, so it
  fires at both opt levels.  The witnesses are
  SDAG-vs-IR (sNaN payload mismatch at output) and SDAG-vs-GISel.
