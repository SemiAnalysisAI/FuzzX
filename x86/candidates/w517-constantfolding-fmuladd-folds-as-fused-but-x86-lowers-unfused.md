# ConstantFolding picks fused mul-add for llvm.fmuladd / constrained.fmuladd, but x86 default lowers them unfused

## Summary
`llvm/lib/Analysis/ConstantFolding.cpp:4095-4111` constant-folds both
`llvm.fmuladd` and `llvm.experimental.constrained.fmuladd` by calling
`APFloat::fusedMultiplyAdd`. But LangRef says (`int_fmuladd`):
> it is unspecified whether rounding will be performed between the
> multiplication and addition steps. **Fusion is not guaranteed, even if the
> target platform supports it.**

So `fmuladd` is allowed to be lowered as `mul; add` (two rounding steps) or
as `fma` (one rounding step). The x86 backend, by default (no `+fma`
feature), lowers `llvm.fmuladd` to `mulsd; addsd` — i.e., the unfused
variant. This means that for some operand triples the IR-time constant fold
and the runtime computation disagree.

## Reproducer (non-strict `llvm.fmuladd`)
```llvm
define double @t1() {
  %r = call double @llvm.fmuladd.f64(
    double 0x3FF0000000000001,        ; 1 + 2^-52
    double 0x3FF0000000000001,        ; 1 + 2^-52
    double 0xBFF0000000000002)        ; -(1 + 2^-51)
  ret double %r
}
declare double @llvm.fmuladd.f64(double, double, double)
```
`opt -O2 -S`:
```
ret double f0x3970000000000000        ; = 2^-104  (fused result)
```

Same IR through `llc -O2 -mtriple=x86_64`:
```asm
; the constant 4.93e-32 = 2^-104 (still the FUSED result that opt baked in)
movsd .LCPI0_0(%rip), %xmm0
```

Now show that an *unfolded* program — same operands, but passed through
volatile reads — produces a different runtime result on the same target:
```llvm
define double @t1(ptr %p) {
  store volatile double 0x3FF0000000000001, ptr %p
  %a = load volatile double, ptr %p
  store volatile double 0x3FF0000000000001, ptr %p
  %b = load volatile double, ptr %p
  store volatile double 0xBFF0000000000002, ptr %p
  %c = load volatile double, ptr %p
  %r = call double @llvm.fmuladd.f64(double %a, double %b, double %c)
  ret double %r
}
```
`llc -O2 -mtriple=x86_64`:
```asm
movabsq $4607182418800017409, %rax       # 0x3FF0000000000001
movq    %rax, (%rdi)
movsd   (%rdi), %xmm0
movq    %rax, (%rdi)
mulsd   (%rdi), %xmm0                    # (1+2^-52) * (1+2^-52) rounds to 1+2^-51
movabsq $-4616189618054758398, %rax      # 0xBFF0000000000002
movq    %rax, (%rdi)
addsd   (%rdi), %xmm0                    # + -(1+2^-51) = 0.0 exactly
retq
```
Runtime result: `0.0`.

So opt+llc on the original IR gives `2^-104`. The same x86 hardware computing
the same operands "as written" gives `0.0`. The IR `llvm.fmuladd` legally
lowers either way; the constant folder picked one interpretation, the
backend picked the other.

## Reproducer (strict `llvm.experimental.constrained.fmuladd`)
```llvm
define double @t1() strictfp {
entry:
  %r = call double @llvm.experimental.constrained.fmuladd.f64(
    double 0x3FF0000000000001,
    double 0x3FF0000000000001,
    double 0xBFF0000000000002,
    metadata !"round.tonearest",
    metadata !"fpexcept.strict") strictfp
  ret double %r
}
declare double @llvm.experimental.constrained.fmuladd.f64(double, double, double, metadata, metadata)
```
`opt -O2 -S`:
```
ret double f0x3970000000000000          ; 2^-104; memory(none); strict side effect dropped
```
x86 backend also lowers the strict fmuladd as `mulsd; addsd`.

## Root cause
`llvm/lib/Analysis/ConstantFolding.cpp:4095-4111`:
```cpp
if (const auto *ConstrIntr = dyn_cast<ConstrainedFPIntrinsic>(Call)) {
  RoundingMode RM = getEvaluationRoundingMode(ConstrIntr);
  APFloat Res = C1;
  APFloat::opStatus St;
  switch (IntrinsicID) {
  default:
    return nullptr;
  case Intrinsic::experimental_constrained_fma:
  case Intrinsic::experimental_constrained_fmuladd:        // <-- shares fma path
    St = Res.fusedMultiplyAdd(C2, C3, RM);
    break;
  }
  ...
}
```
And lines 4125-4129 (non-constrained `fmuladd` shares `fma`'s path):
```cpp
case Intrinsic::fma:
case Intrinsic::fmuladd: {
  APFloat V = C1;
  V.fusedMultiplyAdd(C2, C3, APFloat::rmNearestTiesToEven);
  return ConstantFP::get(Ty, V);
}
```
Both bucket `fmuladd` together with `fma`, but LangRef explicitly states
`fmuladd` may not be fused. The two intrinsics differ exactly in this
guarantee, so they cannot share the constant-fold path.

## Fix sketch
Three workable options:
1. **Refuse to fold** `fmuladd` when the fused and unfused interpretations
   disagree (compute both via `mul` then `add` and via `fusedMultiplyAdd`;
   only fold if bit-equal).
2. **Always use the unfused interpretation** for `fmuladd` (mul then add).
   This matches what x86 default lowering does and is the conservative
   "definitely available everywhere" semantics.
3. **Refuse to fold** unconditionally for `fmuladd` (matching the
   target-dependence in LangRef).

Approach 1 preserves the most folds and is correct: if both fused and
unfused give the same result for a triple, the user gets the same answer no
matter how it's lowered.

## Notes
- `llvm.fma` is correctly folded as fused — its LangRef requires fusion.
- The bug bites any backend that lowers `fmuladd` to a mul+add pair. x86
  without `-mattr=+fma` is the dominant case; even with `+fma`, the cost
  model may still produce unfused mul+add post-codegen.
- For `constrained.fmuladd` this also drops the strictfp side effect (the
  call disappears, attributes flip to `memory(none)`). Compare to my w515
  (which is about FMF nnan/ninf in `simplifyFPOp` — a different code path).
- For non-strict `llvm.fmuladd`, the bug exists in `opt` alone even without
  any user FMF (no nnan/ninf needed).
- I did not find a prior candidate addressing the constant-fold-as-fused
  issue. `w28-fmuladd-cost-emit-mismatch.md` is a different SLP-cost issue.
