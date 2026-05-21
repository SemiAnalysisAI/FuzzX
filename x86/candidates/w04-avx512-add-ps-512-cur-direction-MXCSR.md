# w04: x86_avx512_{add,sub,mul,div}_{ps,pd}_512 → fadd/etc. drops MXCSR rounding mode

File: llvm/lib/Target/X86/X86InstCombineIntrinsic.cpp, lines 2445–2485

## Reasoning

```cpp
case Intrinsic::x86_avx512_add_ps_512:
case Intrinsic::x86_avx512_div_ps_512:
... :
  if (auto *R = dyn_cast<ConstantInt>(II.getArgOperand(2))) {
    if (R->getValue() == 4) {   // _MM_FROUND_CUR_DIRECTION
      V = IC.Builder.CreateFAdd(Arg0, Arg1);
      return IC.replaceInstUsesWith(II, V);
    }
  }
```

The intrinsic with rounding-mode operand == 4 means "use current MXCSR rounding mode". The fold
converts it to a plain IR `fadd`, which LLVM IR treats as "round-to-nearest-even" by default
(unconstrained FP). If the surrounding code temporarily changed MXCSR (e.g., via `_MM_SET_ROUNDING_MODE`
or `fesetround`) before this call, the original intrinsic must honor the changed rounding mode,
while the rewritten `fadd` is free to be folded under the assumption of round-nearest. The same
issue applies to `sub`, `mul`, `div`, and to the masked scalar variants at 2487–2549.

LLVM IR purists will note "unconstrained FP doesn't promise to honor MXCSR." That is technically
true, but the very purpose of these `_round` intrinsics is to give the programmer a way to express
"current MXCSR" inside an `fesetround` region; rewriting to `fadd` defeats that promise. The fix
is to only fold when (a) the function does not access FP env (no `strictfp`, no constrained ops
in scope) **and** (b) the target's default rounding is RNE.

## Concrete IR

```llvm
declare <16 x float> @llvm.x86.avx512.add.ps.512(<16 x float>, <16 x float>, i32 immarg)
declare void @llvm.x86.sse.stmxcsr(ptr)
declare void @llvm.x86.sse.ldmxcsr(ptr)

define <16 x float> @round_toward_zero(<16 x float> %a, <16 x float> %b, ptr %p) {
  ; Set MXCSR to round-toward-zero (bits 13:14 = 11)
  call void @llvm.x86.sse.ldmxcsr(ptr %p)
  ; CUR_DIRECTION = 4
  %r = call <16 x float> @llvm.x86.avx512.add.ps.512(
        <16 x float> %a, <16 x float> %b, i32 4)
  ret <16 x float> %r
}
```

After InstCombine: `%r = fadd <16 x float> %a, %b`. The `fadd` may be constant-folded with
round-to-nearest semantics during subsequent passes, producing a different value than the
hardware addition under round-toward-zero MXCSR.

## Expected wrong result

For `%a = <float 1.0, ...>` and `%b = <float 0x3E70000000000000, ...>` (very small positive),
under round-toward-zero the result is `1.0` exactly; under round-to-nearest-even it rounds up to
`1.0 + ulp(1.0)`. After the fold + later constant fold, the result would be the round-nearest
value while the source program expected the round-toward-zero value from the still-set MXCSR.
