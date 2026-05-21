# w389: CodeGenPrepare::optimizePhiType drops debug locations and metadata when retyping a PHI

**File:** `llvm/lib/CodeGen/CodeGenPrepare.cpp`
**Lines:** 7100-7121 (the PHI/Bitcast creation block inside `CodeGenPrepare::optimizePhiType`)
**Function:** `CodeGenPrepare::optimizePhiType` (defined at line 6986; reached via `CodeGenPrepare::optimizePhiTypes`)
**Severity:** Debug-info loss (`!dbg` on PHI and inserted bitcasts), and silent drop of any non-debug metadata attached to the PHI.

## Summary

`optimizePhiType` is the transform that converts a connected component of `phi i32 ... -> bitcast i32 to float -> store/use` (or a similar pattern with loads on the producer side) into the float-typed equivalent: it creates new float-typed PHIs, inserts bitcasts at producers, and removes the producer-side bitcasts. The transform is enabled by default (`OptimizePhiTypes = true`) and fires on x86 (X86 overrides `shouldConvertPhiType` to return the default in 64-bit mode, lib/Target/X86/X86ISelLowering.cpp:36382-36386).

The bug: when creating the replacement PHIs (lines 7102-7103) and when inserting the new producer-side bitcasts (line 7098), the code does **not** call `setDebugLoc` from the original PHI/Def, and does **not** copy any metadata. The original PHI's `!dbg` is silently dropped, and any custom metadata on the PHI (e.g. `!annotation`, future PHI metadata) is lost.

## Source

```c++
// llvm/lib/CodeGen/CodeGenPrepare.cpp:7096-7121
for (Instruction *D : Defs) {
  if (isa<BitCastInst>(D)) {
    ValMap[D] = D->getOperand(0);
    DeletedInstrs.insert(D);
  } else {
    BasicBlock::iterator insertPt = std::next(D->getIterator());
    ValMap[D] = new BitCastInst(D, ConvertTy, D->getName() + ".bc", insertPt);
    // <-- no setDebugLoc(D->getDebugLoc())
  }
}
for (PHINode *Phi : PhiNodes)
  ValMap[Phi] = PHINode::Create(ConvertTy, Phi->getNumIncomingValues(),
                                Phi->getName() + ".tc", Phi->getIterator());
  // <-- no setDebugLoc(Phi->getDebugLoc()), no copyMetadata(*Phi)
// Pipe together all the PhiNodes.
for (PHINode *Phi : PhiNodes) {
  PHINode *NewPhi = cast<PHINode>(ValMap[Phi]);
  for (int i = 0, e = Phi->getNumIncomingValues(); i < e; i++)
    NewPhi->addIncoming(ValMap[Phi->getIncomingValue(i)],
                        Phi->getIncomingBlock(i));
  Visited.insert(NewPhi);
}
```

## Reproducer (`test_phitype.ll`)

```llvm
target triple = "x86_64-unknown-linux-gnu"

define float @test(i1 %c, ptr %p, ptr %p2) !dbg !6 {
entry:
  br i1 %c, label %a, label %b, !dbg !10

a:
  %v1 = load i32, ptr %p, align 4, !dbg !11
  br label %merge, !dbg !12

b:
  %v2 = load i32, ptr %p2, align 4, !dbg !13
  br label %merge, !dbg !14

merge:
  %phi = phi i32 [ %v1, %a ], [ %v2, %b ], !dbg !15
  %bc = bitcast i32 %phi to float, !dbg !16
  ret float %bc, !dbg !17
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
!17 = !DILocation(line: 9, column: 1, scope: !6)
```

## Reproduce

```
$ /home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/llc \
    -mtriple=x86_64-unknown-linux-gnu -O2 -stop-after=codegenprepare \
    test_phitype.ll -o -
```

## Observed IR after CGP

```llvm
define float @test(i1 %c, ptr %p, ptr %p2) !dbg !5 {
entry:
  br i1 %c, label %a, label %b, !dbg !9

a:                                                ; preds = %entry
  %v1 = load i32, ptr %p, align 4, !dbg !10
  %v1.bc = bitcast i32 %v1 to float                     ; <-- NO !dbg
  br label %merge, !dbg !11

b:                                                ; preds = %entry
  %v2 = load i32, ptr %p2, align 4, !dbg !12
  %v2.bc = bitcast i32 %v2 to float                     ; <-- NO !dbg
  br label %merge, !dbg !13

merge:                                            ; preds = %b, %a
  %phi.tc = phi float [ %v1.bc, %a ], [ %v2.bc, %b ]    ; <-- NO !dbg (original had !15 line 7)
  ret float %phi.tc, !dbg !14
}
```

The original PHI's debug location (`!15`, line 7) and the consumer-side bitcast's debug location (`!16`, line 8) are no longer in the program. Three of the four substitute instructions (`%v1.bc`, `%v2.bc`, `%phi.tc`) have no debug locations at all.

## Suggested fix

After creating each new PHI or bitcast, propagate the debug location and any safe metadata:

```c++
for (Instruction *D : Defs) {
  ...
  } else {
    BasicBlock::iterator insertPt = std::next(D->getIterator());
    auto *BC = new BitCastInst(D, ConvertTy, D->getName() + ".bc", insertPt);
    BC->setDebugLoc(D->getDebugLoc());
    ValMap[D] = BC;
  }
}
for (PHINode *Phi : PhiNodes) {
  auto *NewPhi = PHINode::Create(ConvertTy, Phi->getNumIncomingValues(),
                                 Phi->getName() + ".tc", Phi->getIterator());
  NewPhi->setDebugLoc(Phi->getDebugLoc());
  NewPhi->copyMetadata(*Phi);   // forward !annotation / !pcsections / etc.
  ValMap[Phi] = NewPhi;
}
```

## Impact

- Debug-line attribution is silently wrong for the PHI / load-bitcast pattern that gets retyped — line 7 in the source disappears from the debug info entirely after CGP.
- Any custom metadata attached to the original PHI (`!annotation` from remark passes, future PHI metadata kinds) is dropped silently.
- Matches the recently-filed w200..w209 family of CGP "debug-loc / metadata loss across rewrite" bugs.
