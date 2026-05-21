# w28: SLP FMulAdd CombinedOp cost-vs-emit mismatch

**File**: `llvm/lib/Transforms/Vectorize/SLPVectorizer.cpp:15046-15080, 17098-17120, 22592-22602`

## Reasoning

In `transformNodes`, an FAdd/FSub entry whose FMul operand can be contracted
to `fmuladd` is marked with `E.CombinedOp = TreeEntry::FMulAdd`, and the FMul
child entry is downgraded to `TreeEntry::CombinedVectorize` (so it is skipped
in cost accounting at line 18690). The cost model at line 17098–17120 then
charges the parent the price of one vector `llvm.fmuladd` intrinsic per lane
group (line 17115), effectively giving "free fmul + fma" pricing.

However, the emit side (`vectorizeTree`, line 22591–22602 and the BinOp case
at 23061–23130) has NO case for `TreeEntry::FMulAdd`: only the five
`ReducedBitcast*` opcodes are mapped to a special `ShuffleOrOp`. The FMulAdd
node falls through to the standard `Instruction::FAdd` emit, which produces
a vector `fadd`. The `CombinedVectorize` FMul child is still emitted as a
separate vector `fmul` by recursive `vectorizeTree` (the state only suppresses
cost, not emission — see comment at line 13358). No `Intrinsic::fmuladd` is
ever created.

Result: cost-based vectorization decisions are made on the assumption of FMA
fusion, but the IR produced is plain fmul + fadd. On targets without
hardware FMA (or where the backend's late `combineFMA` fails), this leads to
SLP committing to vectorization that the cost model said was profitable
**only** because of the imagined fmuladd. Worst case is increased cost vs.
scalar; the IR itself is semantically correct.

This is a cost-model bug, not a miscompile, but it can cause SLP to vectorize
unprofitable patterns and regress code on FMA-less x86 targets (e.g.
`-mattr=-fma`).

## IR Repro

```ll
; RUN: opt -passes='slp-vectorizer' -mtriple=x86_64-unknown-linux-gnu -mattr=-fma -S
define void @fma(ptr %a, ptr %b, ptr %c, ptr %d) {
  %a0 = getelementptr float, ptr %a, i32 0
  %a1 = getelementptr float, ptr %a, i32 1
  %a2 = getelementptr float, ptr %a, i32 2
  %a3 = getelementptr float, ptr %a, i32 3
  %b0 = getelementptr float, ptr %b, i32 0
  %b1 = getelementptr float, ptr %b, i32 1
  %b2 = getelementptr float, ptr %b, i32 2
  %b3 = getelementptr float, ptr %b, i32 3
  %c0 = getelementptr float, ptr %c, i32 0
  %c1 = getelementptr float, ptr %c, i32 1
  %c2 = getelementptr float, ptr %c, i32 2
  %c3 = getelementptr float, ptr %c, i32 3
  %d0 = getelementptr float, ptr %d, i32 0
  %d1 = getelementptr float, ptr %d, i32 1
  %d2 = getelementptr float, ptr %d, i32 2
  %d3 = getelementptr float, ptr %d, i32 3

  %la0 = load float, ptr %a0
  %la1 = load float, ptr %a1
  %la2 = load float, ptr %a2
  %la3 = load float, ptr %a3
  %lb0 = load float, ptr %b0
  %lb1 = load float, ptr %b1
  %lb2 = load float, ptr %b2
  %lb3 = load float, ptr %b3
  %lc0 = load float, ptr %c0
  %lc1 = load float, ptr %c1
  %lc2 = load float, ptr %c2
  %lc3 = load float, ptr %c3

  %m0 = fmul contract float %la0, %lb0
  %m1 = fmul contract float %la1, %lb1
  %m2 = fmul contract float %la2, %lb2
  %m3 = fmul contract float %la3, %lb3
  %r0 = fadd contract float %m0, %lc0
  %r1 = fadd contract float %m1, %lc1
  %r2 = fadd contract float %m2, %lc2
  %r3 = fadd contract float %m3, %lc3

  store float %r0, ptr %d0
  store float %r1, ptr %d1
  store float %r2, ptr %d2
  store float %r3, ptr %d3
  ret void
}
```

## Expected Wrong Outcome

Cost model uses `IntrinsicCostAttributes(Intrinsic::fmuladd, ...)` at line
17115 as if a vector fmuladd will be emitted. Output IR contains
`fmul <4 x float>` and `fadd <4 x float>` (no `@llvm.fmuladd`), confirming
the cost discount is unrealized on emission. With `-mattr=-fma`, no
fmuladd-style instruction is selected, so SLP took on real cost it assumed
was free.
