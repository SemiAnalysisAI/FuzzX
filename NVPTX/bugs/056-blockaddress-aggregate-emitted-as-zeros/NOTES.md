# 056 — blockaddress nested in an aggregate global is silently emitted as zeros (miscompile)

- **Kind:** miscompile
- **Reachable via:** default llc
- **Component:** NVPTXAsmPrinter.cpp 1703-1711 (bufferLEByte, case Type::PointerTyID)  (round-8 area `X01-asmprinter-ptr-const`)

## Summary

a `blockaddress` nested in an aggregate global is silently emitted as all-zero bytes (the block-address relocation is dropped)

## Mechanism / root cause

In bufferLEByte the PointerTyID case is:
  if (const GlobalValue *GVar = dyn_cast<GlobalValue>(CPV))
    AggBuffer->addSymbol(GVar, GVar);
  else if (const ConstantExpr *Cexpr = dyn_cast<ConstantExpr>(CPV)) { ... addSymbol ... }
  AggBuffer->addZeros(AllocSize);
A `BlockAddress` pointer element is neither a GlobalValue nor a ConstantExpr, so NEITHER branch runs: no symbol is recorded and only `addZeros(AllocSize)` executes. The aggregate then has numSymbols()==0 and is emitted on the plain `.b8` path with trailing zeros trimmed, producing all-zero bytes where the block address should be. For `@ba_arr = global [1 x ptr] [ptr blockaddress(@bar,%lbl)]` NVPTX emits `.b8 ba_arr[8] = {}` (all zero); x86 emits `.quad .Ltmp0`. Same defect via the packed-struct `.u8 mask()` path: `<{ i8 7, ptr blockaddress(...) }>` emits `.b8 pkb[9] = {7}` — the 8 pointer bytes are silently dropped. Silent wrong constant, no diagnostic.

## Trigger

A `blockaddress` pointer element nested inside an array/vector or (packed) struct global initializer; e.g. `[1 x ptr] [ptr blockaddress(@f,%bb)]` or `<{ i8, ptr }> <{ i8 7, ptr blockaddress(@f,%bb) }>`.

## Reproducer

```
define void @bar() {
entry:
  br label %lbl
lbl:
  ret void
}
@ba_arr = global [1 x ptr] [ptr blockaddress(@bar, %lbl)]
```

Command: `llc -mtriple=nvptx64-nvidia-cuda -mcpu=sm_70 -o - repro.ll`

## Verification

Reproduced with the built llc (crash/emitted-PTX matches; finder confidence 0.95). For the blockaddress-as-zeros and non-pow2-vector cases the wrong layout was cross-checked against x86, which emits the correct bytes.
