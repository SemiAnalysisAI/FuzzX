# 062 — External global of an opaque / unsized type crashes the AsmPrinter (getAlignment assert on unsized type)

- **Kind:** crash (assert/UB)
- **Reachable via:** default llc
- **Component:** NVPTXAsmPrinter.cpp 1298-1353 (emitPTXGlobalVariable); reached via printModuleLevelGV line 926-932  (round-8 area `X02-asmprinter-struct-pad`)

## Summary

an `external global` of an opaque/unsized type crashes `emitPTXGlobalVariable` (DataLayout `getAlignment` assert on an unsized type)

## Mechanism / root cause

printModuleLevelGV emits `.extern ` (line 904) for an external-linkage GlobalVariable with no initializer, then for a declaration (GVar->isDeclaration(), line 926) calls emitPTXGlobalVariable(GVar). That function does:
  Type *ETy = GVar->getValueType();
  ...
  O << " .align " << GVar->getAlign().value_or(DL.getPrefTypeAlign(ETy)).value();   // line 1315-1316
  ...
  case StructTyID/ArrayTyID/FixedVectorTyID: ElementSize = DL.getTypeStoreSize(ETy); // line 1342
Both getPrefTypeAlign(ETy) and getTypeStoreSize(ETy) call DataLayout::getAlignment / getTypeSizeInBits, which assert `Ty->isSized()`. For an opaque (incomplete) struct type the type is unsized, so the assert fires:
  Assertion failed: (Ty->isSized() && "Cannot getTypeInfo() on a type that is unsized!"), function getAlignment, DataLayout.cpp:872
(Confirmed in stack: frame #7 DataLayout::getAlignment <- #8 NVPTXAsmPrinter::emitPTXGlobalVariable.) Adding an explicit `, align 4` does not help: getPrefTypeAlign is then skipped but getTypeStoreSize(ETy) at line 1342 hits the same assert.

Why wrong / a bug: `%opaque = type opaque` with `@g = external global %opaque` is valid IR (verifier passes), corresponding to `extern struct Incomplete g;`. The target-independent path (x86) emits nothing for a pure external declaration and does not crash; NVPTX crashes. emitPTXGlobalVariable never checks ETy->isSized() before querying alignment/size.

## Trigger

A module-scope external (declaration, no initializer) global whose value type is an opaque/unsized struct type. opt -passes=verify accepts the IR (exit 0).

## Reproducer

```
target triple = "nvptx64-nvidia-cuda"
%opaque = type opaque
@g = external global %opaque
```

Command: `llc -mtriple=nvptx64-nvidia-cuda -o - repro.ll`

## Verification

Reproduced with the built llc (crash/emitted-PTX matches; finder confidence 0.85). 
