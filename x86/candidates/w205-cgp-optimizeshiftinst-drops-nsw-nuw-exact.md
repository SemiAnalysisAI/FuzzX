# w205: CodeGenPrepare::optimizeShiftInst drops nsw/nuw/exact flags

**File:** `llvm/lib/CodeGen/CodeGenPrepare.cpp`
**Lines:** 7656-7687
**Function:** `CodeGenPrepare::optimizeShiftInst`

## Summary
When CGP hoists a vector shift over a select-of-splatted scalars (`shift X, (select C, T, F)` -> `select C, (shift X, T), (shift X, F)`), the two new `Builder.CreateBinOp` shifts are created with no flags. The original shift's `nsw`/`nuw`/`exact` poison-generating flags are silently dropped. Subsequent passes that consume the new shifts can no longer infer the original no-overflow / exact contracts.

This is a flag-loss bug: it does not by itself create UB, but it loses optimization information that the IR Producer expressed.

## Source

```c++
IRBuilder<> Builder(Shift);
BinaryOperator::BinaryOps Opcode = Shift->getOpcode();
Value *NewTVal = Builder.CreateBinOp(Opcode, Shift->getOperand(0), TVal);
Value *NewFVal = Builder.CreateBinOp(Opcode, Shift->getOperand(0), FVal);
Value *NewSel = Builder.CreateSelect(Cond, NewTVal, NewFVal);
replaceAllUsesWith(Shift, NewSel, FreshBBs, IsHugeFunc);
Shift->eraseFromParent();
```

Neither `NewTVal` nor `NewFVal` calls `copyIRFlags(Shift)` or otherwise propagates the `nsw`/`nuw`/`exact` flags that `Shift` may carry.

## Reproducer (`test_shiftinst.ll`)

```llvm
target triple = "x86_64-unknown-linux-gnu"

define <4 x i32> @test(<4 x i32> %x, i1 %c, i32 %t, i32 %f) {
entry:
  %tv = insertelement <4 x i32> poison, i32 %t, i32 0
  %ts = shufflevector <4 x i32> %tv, <4 x i32> poison, <4 x i32> zeroinitializer
  %fv = insertelement <4 x i32> poison, i32 %f, i32 0
  %fs = shufflevector <4 x i32> %fv, <4 x i32> poison, <4 x i32> zeroinitializer
  %sel = select i1 %c, <4 x i32> %ts, <4 x i32> %fs
  %r = shl nsw nuw <4 x i32> %x, %sel
  ret <4 x i32> %r
}
```

The shift is `shl nsw nuw`. The select-of-splats matches the pattern in `optimizeShiftInst`, and on default x86 (SSE2-only), `isVectorShiftByScalarCheap(v4i32)` returns true (`X86TargetTransformInfo.cpp:7240`).

## Reproduce
```
$ /home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/llc \
    -mtriple=x86_64-unknown-linux-gnu -stop-after=codegenprepare \
    test_shiftinst.ll -o -
```

Observed IR after CGP:
```llvm
%0 = shl <4 x i32> %x, %ts
%1 = shl <4 x i32> %x, %fs
%2 = select i1 %c, <4 x i32> %0, <4 x i32> %1
```

Neither `nsw` nor `nuw` is present. Compared to the input (`shl nsw nuw <4 x i32> %x, %sel`), both flags have been dropped.

## Suggested fix
After creating each `NewTVal` / `NewFVal`, copy IR flags from the original:
```c++
if (auto *I = dyn_cast<Instruction>(NewTVal)) I->copyIRFlags(Shift);
if (auto *I = dyn_cast<Instruction>(NewFVal)) I->copyIRFlags(Shift);
```

## Impact
Lost optimization opportunity for downstream consumers (DAGCombine, etc.) that rely on `nsw`/`nuw`/`exact` to fold shifts into other operations or to prove non-overflow.

The matching `optimizeFunnelShift` (lines 7689-7722) has the analogous bug for funnel shifts: `Builder.CreateIntrinsic(Opcode, Ty, ...)` does not copy fast-math flags or any other metadata from `Fsh`.
