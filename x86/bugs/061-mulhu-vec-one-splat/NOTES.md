# Candidate: visitMULHU vector splat-1 produces undef via shift-by-bitwidth

File: llvm/lib/CodeGen/SelectionDAG/DAGCombiner.cpp:5685-5704

## Reasoning

`visitMULHU` has the scalar early-out `isOneConstant(N1) -> 0` at line 5685,
but `isOneConstant` is scalar-only. For a uniform vector splat `<1, 1, ...>`,
that fold doesn't fire. We fall through to the `mulhu x, (1 << c) -> x >> (bitwidth - c)`
fold at lines 5692-5704. `BuildLogBase2(<1,1>) = 0`, so `SRLAmt = NumEltBits - 0 = NumEltBits`.
A SRL by exactly the element bitwidth is documented UB (`simplifyShift` turns
it into UNDEF). The original `mulhu(x, 1)` must be `0`, but the rewrite produces
an UNDEF SRL. Replacing a guaranteed-zero value with undef is a refinement
regression (the backend may now pick any value, including non-zero, breaking
downstream code that relied on the zero).

## Candidate IR (llc -mtriple=x86_64)

```ll
define <4 x i32> @bug(<4 x i32> %x) {
  %r = call <4 x i32> @llvm.x86.umul.fix.v4i32(<4 x i32> %x,
                                               <4 x i32> <i32 1, i32 1, i32 1, i32 1>,
                                               i32 32)
  ret <4 x i32> %r
}
```

A more direct trigger via SDAG: any pattern that generates `ISD::MULHU` with a
build_vector constant `<1, 1, ...>` operand. Targets that don't legalize MULHU
to a multiply (and where the fold runs) will exhibit it.

Note: vector ISD::MULHU is often custom-lowered or expanded on x86; the fold
runs before legalization, so a pre-legalize MULHU with vector splat-1 is the
trigger. May be more reproducible on AArch64/RISCV with vector ISA, but x86
SSE2 has PMULHUW (i16 lane) so a `<8 x i16>` mulhu by splat 1 is the prime case.

## Expected wrong outcome

Result `<0, 0, ...>` becomes a freely-chosen value (e.g., garbage register
contents), observable as a non-zero result in a follow-on store/compare.
