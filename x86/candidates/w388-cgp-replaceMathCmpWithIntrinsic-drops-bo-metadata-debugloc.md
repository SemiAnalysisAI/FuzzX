# w388: CodeGenPrepare::replaceMathCmpWithIntrinsic drops metadata and debug-loc of original BO

**File:** `llvm/lib/CodeGen/CodeGenPrepare.cpp`
**Lines:** 1584-1668 (`replaceMathCmpWithIntrinsic`), with the offending IR-builder block at 1638-1664
**Function:** `CodeGenPrepare::replaceMathCmpWithIntrinsic`, reached from `combineToUAddWithOverflow` (line 1722) and `combineToUSubWithOverflow` (line 1791).
**Severity:** Metadata loss + debug-loc loss.

## Summary

When CGP combines an `add`/`sub` plus its overflow-check `icmp` into an `{u,s}add/sub.with.overflow` intrinsic call, the IR builder is positioned at the **first of the two instructions** (cmp or BO):

```c++
Instruction *InsertPt = nullptr;
for (Instruction &Iter : *Cmp->getParent()) {
  if ((BO->getOpcode() != Instruction::Xor && &Iter == BO) || &Iter == Cmp) {
    InsertPt = &Iter;
    break;
  }
}
...
IRBuilder<> Builder(InsertPt);
Value *MathOV = Builder.CreateBinaryIntrinsic(IID, Arg0, Arg1);
Value *Math   = Builder.CreateExtractValue(MathOV, 0, "math");
replaceAllUsesWith(BO, Math, FreshBBs, IsHugeFunc);
...
Value *OV     = Builder.CreateExtractValue(MathOV, 1, "ov");
replaceAllUsesWith(Cmp, OV, FreshBBs, IsHugeFunc);
```

Issues:
1. **Metadata loss on BO.** The original `add`/`sub` can carry `!annotation`, `!nontemporal`, or future custom metadata. The new intrinsic call and the `extractvalue` that replaces `BO` do not copy any of it.
2. **Debug-loc loss.** `IRBuilder<>(InsertPt)` adopts `InsertPt`'s debug location. If `BO` and `Cmp` have different debug locations and `Cmp` happens to be earlier in the block, `BO`'s debug location is silently dropped from the resulting `Math` value (which is the user-visible replacement for `BO`'s SSA value).
3. **IR flag loss on BO.** An `add nuw` becoming `uadd.with.overflow` loses the `nuw` flag. The new intrinsic does not carry an equivalent assertion; downstream consumers that walk the extracted-value scalar lose the no-overflow fact the producer expressed. (The intrinsic call form does at least represent the overflow bit, but for a `nuw add`, that bit is provably zero — analyzers that previously relied on `nuw` no longer have that assertion.)

This is the analog for overflow-intrinsic conversion of the w205..w209 series of CGP IR-flag/metadata-loss bugs.

## Source

```c++
// llvm/lib/CodeGen/CodeGenPrepare.cpp:1638-1668
// Insert at the first instruction of the pair.
Instruction *InsertPt = nullptr;
for (Instruction &Iter : *Cmp->getParent()) {
  if ((BO->getOpcode() != Instruction::Xor && &Iter == BO) || &Iter == Cmp) {
    InsertPt = &Iter;
    break;
  }
}
assert(InsertPt != nullptr && "Parent block did not contain cmp or binop");

IRBuilder<> Builder(InsertPt);
Value *MathOV = Builder.CreateBinaryIntrinsic(IID, Arg0, Arg1);
if (BO->getOpcode() != Instruction::Xor) {
  Value *Math = Builder.CreateExtractValue(MathOV, 0, "math");
  replaceAllUsesWith(BO, Math, FreshBBs, IsHugeFunc);
} else
  assert(BO->hasOneUse() &&
         "Patterns with XOr should use the BO only in the compare");
Value *OV = Builder.CreateExtractValue(MathOV, 1, "ov");
replaceAllUsesWith(Cmp, OV, FreshBBs, IsHugeFunc);
```

No `copyMetadata(*BO, ...)`, no `MathOV->setDebugLoc(BO->getDebugLoc())`, no copying of `BO`'s IR flags onto a downstream wrapper.

## Reproducer (`test_uaddo.ll`)

```llvm
target triple = "x86_64-unknown-linux-gnu"

define { i32, i1 } @test(i32 %a, i32 %b) {
entry:
  %add = add nuw i32 %a, %b, !annotation !0
  %cmp = icmp ult i32 %add, %a
  %s = insertvalue { i32, i1 } poison, i32 %add, 0
  %r = insertvalue { i32, i1 } %s, i1 %cmp, 1
  ret { i32, i1 } %r
}

!0 = !{!"test_annotation_add"}
```

## Reproduce

```
$ /home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/llc \
    -mtriple=x86_64-unknown-linux-gnu -O2 -stop-after=codegenprepare \
    test_uaddo.ll -o -
```

## Observed IR after CGP

```llvm
define { i32, i1 } @test(i32 %a, i32 %b) {
entry:
  %0 = call { i32, i1 } @llvm.uadd.with.overflow.i32(i32 %a, i32 %b)
  %math = extractvalue { i32, i1 } %0, 0
  %ov = extractvalue { i32, i1 } %0, 1
  %s = insertvalue { i32, i1 } poison, i32 %math, 0
  %r = insertvalue { i32, i1 } %s, i1 %ov, 1
  ret { i32, i1 } %r
}
```

- `!annotation !0` from the original `add` is gone.
- The `nuw` assertion (the producer guaranteed no unsigned overflow) is gone — downstream analyses cannot recover it from the new `uadd.with.overflow` (the overflow bit may be 0, but that has to be re-proved each time).
- The new instructions have no debug locations from `add`/`icmp`.

## Suggested fix

```c++
IRBuilder<> Builder(InsertPt);
auto *MathOV = cast<Instruction>(
    Builder.CreateBinaryIntrinsic(IID, Arg0, Arg1));
MathOV->setDebugLoc(BO->getDebugLoc());
MathOV->copyMetadata(*BO,
    {LLVMContext::MD_annotation, LLVMContext::MD_pcsections,
     LLVMContext::MD_dbg});

if (BO->getOpcode() != Instruction::Xor) {
  Value *Math = Builder.CreateExtractValue(MathOV, 0, "math");
  if (auto *MI = dyn_cast<Instruction>(Math))
    MI->setDebugLoc(BO->getDebugLoc());
  replaceAllUsesWith(BO, Math, FreshBBs, IsHugeFunc);
}
Value *OV = Builder.CreateExtractValue(MathOV, 1, "ov");
if (auto *OI = dyn_cast<Instruction>(OV))
  OI->setDebugLoc(Cmp->getDebugLoc());
replaceAllUsesWith(Cmp, OV, FreshBBs, IsHugeFunc);
```

## Impact

- Programmer-attached metadata (PGO remarks, sanitizer annotations) is dropped.
- Source-level debugger attribution for the math operation moves to whichever of (`BO`, `Cmp`) appears first in the block, silently changing source-line attribution.
- Lost `nuw`/`nsw` IR-flag assertion: downstream code (after CGP) that previously had `add nuw` now has `extractvalue { i32, i1 } %uaddo, 0` with no equivalent guarantee, requiring re-derivation.

This pattern matches w205..w209 "CGP loses IR flags / metadata during rewrites".
