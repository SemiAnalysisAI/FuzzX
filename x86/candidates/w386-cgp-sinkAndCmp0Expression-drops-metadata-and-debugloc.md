# w386: CodeGenPrepare::sinkAndCmp0Expression drops metadata on sunk `and`

**File:** `llvm/lib/CodeGen/CodeGenPrepare.cpp`
**Lines:** 2300-2369 (`sinkAndCmp0Expression`), with the offending insertion at 2354-2358
**Function:** `sinkAndCmp0Expression`
**Severity:** Metadata loss (non-debug).

## Summary

When CGP sinks an `and` feeding `icmp X, 0` into each user's basic block (so the per-block test becomes a single TEST/AND-with-flags), it constructs each sunk copy with `BinaryOperator::Create(Instruction::And, AndI->getOperand(0), AndI->getOperand(1), "", InsertPt->getIterator())`. Only `setDebugLoc(AndI->getDebugLoc())` is then called on the new instruction. Any other metadata attached to the original `AndI` — for example `!annotation` (LLVM remarks/PGO) or any future custom metadata — is silently dropped.

Note: although `and` itself has no poison-generating IR flags (no `nsw`/`nuw`/`exact`/`disjoint`), it is a real instruction that can legitimately carry attached metadata. The bug is metadata loss, not flag loss.

## Source

```c++
// llvm/lib/CodeGen/CodeGenPrepare.cpp:2351-2358
Instruction *InsertPt =
    User->getParent() == AndI->getParent() ? AndI : User;
Instruction *InsertedAnd = BinaryOperator::Create(
    Instruction::And, AndI->getOperand(0), AndI->getOperand(1), "",
    InsertPt->getIterator());
// Propagate the debug info.
InsertedAnd->setDebugLoc(AndI->getDebugLoc());

// Replace a use of the 'and' with a use of the new 'and'.
TheUse = InsertedAnd;
```

No `copyMetadata(*AndI, ...)` call.

## Reproducer (`test_sinkand.ll`)

```llvm
target triple = "x86_64-unknown-linux-gnu"

define i32 @test(i64 %x, i1 %c, i32 %arg) {
entry:
  ; Use constant for one operand so we pass the "register pressure" gate at
  ; line 2315-2318 of sinkAndCmp0Expression.
  %and = and i64 %x, 255, !annotation !0
  br i1 %c, label %if.then, label %if.else

if.then:
  %tst1 = icmp eq i64 %and, 0
  %r1 = zext i1 %tst1 to i32
  br label %if.end

if.else:
  %tst2 = icmp ne i64 %and, 0
  %r2 = zext i1 %tst2 to i32
  br label %if.end

if.end:
  %phi = phi i32 [ %r1, %if.then ], [ %r2, %if.else ]
  ret i32 %phi
}

!0 = !{!"some_annotation"}
```

## Reproduce

```
$ /home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/llc \
    -mtriple=x86_64-unknown-linux-gnu -O2 -stop-after=codegenprepare \
    test_sinkand.ll -o -
```

## Observed IR after CGP

```llvm
define i32 @test(i64 %x, i1 %c, i32 %arg) {
entry:
  br i1 %c, label %if.then, label %if.else

if.then:                                          ; preds = %entry
  %0 = and i64 %x, 255                            ; <-- !annotation dropped
  %tst1 = icmp eq i64 %0, 0
  ...

if.else:                                          ; preds = %entry
  %1 = and i64 %x, 255                            ; <-- !annotation dropped
  %tst2 = icmp ne i64 %1, 0
  ...
}
```

The original `and` carried `!annotation !0` (per `!0 = !{!"some_annotation"}`). Both sunk copies have stripped the metadata silently. `MD_annotation` is a non-trivial metadata kind that pass implementers explicitly want to preserve across CGP rewrites (it is preserved by `Instruction::copyMetadata` / `combineMetadataForCSE`).

## Suggested fix

Add a `copyMetadata` call (with the standard "safe-to-copy" filter) right after creating the new `and`:

```c++
Instruction *InsertedAnd = BinaryOperator::Create(
    Instruction::And, AndI->getOperand(0), AndI->getOperand(1), "",
    InsertPt->getIterator());
InsertedAnd->setDebugLoc(AndI->getDebugLoc());
// Forward any safe metadata (e.g. !annotation).
InsertedAnd->copyMetadata(*AndI,
    {LLVMContext::MD_annotation, LLVMContext::MD_pcsections});
```

## Impact

- LLVM remark / PGO metadata attached to the original `and` is silently dropped when CGP sinks the and. Diagnostics produced after CGP no longer have a back-reference to the original IR construct.
- Custom IR-instrumentation pipelines (e.g. coverage / sanitizer instrumentation that tags `and` operations with `!annotation`) lose information they rely on.

Related code:
- The companion `sinkCmpExpression` (lines 1874-1945) has the identical issue for the cmp metadata (already filed as w207).
- The IR-flag analog of this bug is filed as w205 / w206 for `optimizeShiftInst` / `optimizeFunnelShift`.
