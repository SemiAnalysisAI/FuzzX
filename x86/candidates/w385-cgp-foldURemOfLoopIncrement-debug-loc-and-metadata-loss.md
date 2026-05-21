# w385: CodeGenPrepare::foldURemOfLoopIncrement drops debug locations and metadata from replaced urem

**File:** `llvm/lib/CodeGen/CodeGenPrepare.cpp`
**Lines:** 2186-2262 (`foldURemOfLoopIncrement`), with the offending IR-builder block at 2235-2249
**Function:** `foldURemOfLoopIncrement` (called from `CodeGenPrepare::optimizeURem`, line 2264)
**Severity:** Debug-info loss + general metadata loss when CGP rewrites `urem(IV, M)` into a recurrence using `add nuw` + `icmp eq` + `select`.

## Summary

When CGP rewrites an in-loop `urem(LoopIncrPHI, RemAmt)` (or `urem(add nuw LoopIncrPHI, K, RemAmt)`) into a recurrence-form that keeps `0 .. RemAmt-1` in a fresh PHI, all the newly-created instructions (the PHI itself, the `add nuw`, the `icmp eq`, and the `select`) inherit their debug locations from `IRBuilder<>::SetInsertPoint`'s default behavior — i.e. from the **insertion point** rather than from the `Rem` instruction that is being replaced.

In particular:
- `Builder.SetInsertPoint(LoopIncrPN)` (line 2237) makes the new PHI use `LoopIncrPN`'s stable debug location, not `Rem`'s.
- `Builder.SetInsertPoint(cast<Instruction>(LoopIncrPN->getIncomingValueForBlock(L->getLoopLatch())))` (lines 2240-2241) makes the new `add nuw`, `icmp eq`, and `select` use the IV-increment's debug location.

The `urem`'s original debug location and metadata (`!range`, `!annotation`, branch-weight props on its users, etc.) are silently dropped. The `Rem->eraseFromParent()` at line 2258 removes the urem with no debug-location transfer to its replacement.

## Source

```c++
// llvm/lib/CodeGen/CodeGenPrepare.cpp:2235-2258
Type *Ty = Rem->getType();
IRBuilder<> Builder(Rem->getContext());

Builder.SetInsertPoint(LoopIncrPN);
PHINode *NewRem = Builder.CreatePHI(Ty, 2);

Builder.SetInsertPoint(cast<Instruction>(
    LoopIncrPN->getIncomingValueForBlock(L->getLoopLatch())));
// `(add (urem x, y), 1)` is always nuw.
Value *RemAdd = Builder.CreateNUWAdd(NewRem, ConstantInt::get(Ty, 1));
Value *RemCmp = Builder.CreateICmp(ICmpInst::ICMP_EQ, RemAdd, RemAmt);
Value *RemSel =
    Builder.CreateSelect(RemCmp, Constant::getNullValue(Ty), RemAdd);

NewRem->addIncoming(Start, L->getLoopPreheader());
NewRem->addIncoming(RemSel, L->getLoopLatch());

...
replaceAllUsesWith(Rem, NewRem, FreshBBs, IsHuge);
Rem->eraseFromParent();
```

No `setDebugLoc(Rem->getDebugLoc())` or `copyMetadata(*Rem)` call.

## Reproducer (`test_urem_loop.ll`)

```llvm
target triple = "x86_64-unknown-linux-gnu"

define void @test(i64 %n, ptr %mp, ptr %out) !dbg !6 {
entry:
  %m = load i64, ptr %mp, align 8, !dbg !10
  br label %loop, !dbg !10

loop:
  %iv = phi i64 [ 0, %entry ], [ %iv.next, %loop ]
  %r = urem i64 %iv, %m, !dbg !12             ; <-- !12 = line 4
  store i64 %r, ptr %out, align 8, !dbg !13
  %iv.next = add nuw i64 %iv, 1, !dbg !11    ; <-- !11 = line 3
  %cmp = icmp ult i64 %iv.next, %n, !dbg !14
  br i1 %cmp, label %loop, label %exit, !dbg !15

exit:
  ret void, !dbg !16
}

!llvm.dbg.cu = !{!0}
!llvm.module.flags = !{!3, !4, !5}
!0 = distinct !DICompileUnit(language: DW_LANG_C99, file: !1, producer: "test", isOptimized: true, runtimeVersion: 0, emissionKind: FullDebug)
!1 = !DIFile(filename: "test.c", directory: "/tmp")
!2 = !{}
!3 = !{i32 7, !"Dwarf Version", i32 4}
!4 = !{i32 2, !"Debug Info Version", i32 3}
!5 = !{i32 1, !"wchar_size", i32 4}
!6 = distinct !DISubprogram(name: "test", scope: !1, file: !1, line: 1, type: !7, scopeLine: 1, spFlags: DISPFlagDefinition, unit: !0, retainedNodes: !2)
!7 = !DISubroutineType(types: !8)
!8 = !{null}
!10 = !DILocation(line: 2, column: 1, scope: !6)
!11 = !DILocation(line: 3, column: 1, scope: !6)
!12 = !DILocation(line: 4, column: 1, scope: !6)
!13 = !DILocation(line: 5, column: 1, scope: !6)
!14 = !DILocation(line: 6, column: 1, scope: !6)
!15 = !DILocation(line: 7, column: 1, scope: !6)
!16 = !DILocation(line: 8, column: 1, scope: !6)
```

## Reproduce

```
$ /home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/llc \
    -mtriple=x86_64-unknown-linux-gnu -O2 -stop-after=codegenprepare \
    test_urem_loop.ll -o -
```

## Observed IR after CGP

```llvm
loop:                                             ; preds = %loop, %entry
  %0 = phi i64 [ 0, %entry ], [ %3, %loop ]           ; <-- NO !dbg
  %iv = phi i64 [ 0, %entry ], [ %iv.next, %loop ]
  store i64 %0, ptr %out, align 8, !dbg !10           ; !10 = line 5 (store dbg)
  %1 = add nuw i64 %0, 1, !dbg !11                    ; !11 = line 3 (IV add dbg, NOT urem)
  %2 = icmp eq i64 %1, %m, !dbg !11                   ; !11 = line 3
  %3 = select i1 %2, i64 0, i64 %1, !dbg !11          ; !11 = line 3
  %iv.next = add nuw i64 %iv, 1, !dbg !11
  ...
```

Compare to the original: the `urem` at `!12` (line 4) is gone; line 4 no longer appears in any output DILocation. The PHI (`%0`) has no debug location at all. The `add nuw`, `icmp eq`, and `select` all got the **IV-increment's** debug location (line 3), not the urem's (line 4).

The urem's debug location (line 4) is silently dropped. Any user-source-correlated debugger would now associate the computation of `%r` with line 3, not line 4. Source-level coverage and PGO mapping are degraded.

## Suggested fix

Set the debug location explicitly on the new instructions:

```c++
DebugLoc DL = Rem->getDebugLoc();
Builder.SetInsertPoint(LoopIncrPN);
PHINode *NewRem = Builder.CreatePHI(Ty, 2);
NewRem->setDebugLoc(DL);

Builder.SetInsertPoint(cast<Instruction>(
    LoopIncrPN->getIncomingValueForBlock(L->getLoopLatch())));
Builder.SetCurrentDebugLocation(DL);                 // <-- add this
Value *RemAdd = Builder.CreateNUWAdd(NewRem, ConstantInt::get(Ty, 1));
Value *RemCmp = Builder.CreateICmp(ICmpInst::ICMP_EQ, RemAdd, RemAmt);
Value *RemSel = Builder.CreateSelect(RemCmp, Constant::getNullValue(Ty), RemAdd);
```

Optionally `copyMetadata(*Rem, ...)` for non-debug metadata that may have been attached to the urem.

## Impact

- Debug-line attribution is silently wrong for the replacement of the urem.
- Sample-PGO and source-level coverage tools see column / line 3 (IV update) as the source of the recurrence value, not line 4 (urem).
- Any metadata attached to the urem (`!annotation`, future LLVM additions) is dropped silently with no warning.

This matches the pattern of the recently-found w200..w209 series of CGP "metadata/debugloc-loss" bugs and the upstream emphasis on preserving debug locations across IR-rewriting transforms.
