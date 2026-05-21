## Strict FP_EXTEND f16->!f32 drops inner chain, breaking strict-fp ordering

`llvm/lib/Target/X86/X86ISelLowering.cpp:22977-22981` (`X86TargetLowering::LowerFP_EXTEND`)

```cpp
if (VT != MVT::f32) {
  if (IsStrict)
    return DAG.getNode(
        ISD::STRICT_FP_EXTEND, DL, {VT, MVT::Other},
        {Chain, DAG.getNode(ISD::STRICT_FP_EXTEND, DL,
                            {MVT::f32, MVT::Other}, {Chain, In})});
```

When lowering a strict `fpext half -> double` (or `half -> fp128`) on a non-FP16
target, the outer `STRICT_FP_EXTEND` is built with chain input = the original
`Chain`. The inner `STRICT_FP_EXTEND`'s chain output (`.getValue(1)`) is dropped
on the floor. The outer's chain input should be the inner's chain output to
sequence the two strict-fp ops with respect to other chain consumers that also
hang off `Chain`. As written, three nodes are siblings rooted at `Chain` (inner,
outer, and any other strict-fp op of the caller); data dependence forces
inner-before-outer, but a third strict op sharing `Chain` may legally schedule
between them or even reorder around the inner side-effect, since no chain edge
ties it to the inner. This violates the ordering guarantee of strict-fp:
exceptions raised by the inner half->float conversion can be reordered with
other constrained ops in the same BB.

### Candidate IR (X86, no FP16/F16C variant matters because of triple test)

```
define double @f(half %x, float %y) #0 {
  %a = call double @llvm.experimental.constrained.fpext.f64.f16(
         half %x,
         metadata !"fpexcept.strict")
  %b = call float @llvm.experimental.constrained.fadd.f32(
         float %y, float %y,
         metadata !"round.tonearest", metadata !"fpexcept.strict")
  %c = fpext float %b to double
  %r = fadd double %a, %c
  ret double %r
}
attributes #0 = { strictfp }
```

Compile with `llc -mtriple=x86_64-- -mattr=+sse2` (or +avx, NOT +avx512fp16).

### Expected wrong outcome

The inner STRICT_FP_EXTEND (half->float) and the strict FADD share the same
`Chain` input. With nothing tying the FADD's chain after the inner extend,
scheduling/legalization may reorder the FE exception raised by the half->float
conversion past the fadd, or vice versa — observable if the half value is a
sNaN (raising invalid) while the fadd would otherwise raise inexact in a
specific order. The contract of constrained intrinsics is that side effects
appear in program order; this lowering doesn't preserve that across the two
chained extends.

### Cross-reference

`llvm/test/CodeGen/X86/fp-strict-scalar-fp16.ll:252` (`fpext_f16_to_f64`)
exercises this code path on SSE2/AVX but only CHECKs the instruction sequence
for a single conversion; it does not place another chain-bearing strict op
adjacent to verify ordering, so the bug is not covered.
