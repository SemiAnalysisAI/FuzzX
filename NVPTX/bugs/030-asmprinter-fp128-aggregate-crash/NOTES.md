# 030 — bufferLEByte crashes (llvm_unreachable "unsupported type") on fp128 element inside an array or struct

- **Kind:** crash (abort/UB)
- **Reachable via:** default llc
- **Component:** NVPTXAsmPrinter.cpp 1696-1701,1728-1729  (round-5 area `V11-asmprinter-const-more`)
- **Candidate id:** r5_03

## Summary

an `fp128` element nested in an array/struct global hits `llvm_unreachable("unsupported type")` in bufferLEByte (top-level fp128 is fine)

## Mechanism / root cause

bufferLEByte's floating-point switch handles only HalfTyID/BFloatTyID/FloatTyID/DoubleTyID (lines 1696-1701); FP128TyID is absent, so an fp128 Constant reaches `default: llvm_unreachable("unsupported type")` at line 1729. A bare top-level fp128 global works because printModuleLevelGV routes FP128TyID straight to bufferAggregateConstant, which has a dedicated FP128 branch (lines 1759-1765) that emits the 16 bytes. But an fp128 nested in a ConstantArray/ConstantStruct is emitted element-by-element via bufferLEByte (bufferAggregateConstant lines 1768-1799), so each fp128 element hits the unreachable. The bare-fp128 support proves fp128 globals are intended to be supported; the nested case is an omission, not genuine non-support. x86 reference emits the fp128 array/struct bytes correctly.

## Trigger

A global array or struct containing an fp128 element/field, e.g. `[2 x fp128]` or `{ i32, fp128 }`. Both reproduced as crashes.

## Reproducer

```
target triple = "nvptx64-nvidia-cuda"
@arr = addrspace(1) global [2 x fp128] [fp128 0xL00000000000000003FFF000000000000, fp128 0xL00000000000000004000000000000000]
```

Command: `llc -mtriple=nvptx64 -mcpu=sm_90 -o - repro.ll`

## Observed (wrong) output

```
unsupported type
UNREACHABLE executed at /Users/justinlebar/code/llvm2/llvm/lib/Target/NVPTX/NVPTXAsmPrinter.cpp:1729!
PLEASE submit a bug report to https://github.com/llvm/llvm-project/issues/ and include the crash backtrace and instructions to reproduce the bug.
Stack dump:
0.	Program arguments: /Users/justinlebar/code/llvm2/build/bin/llc -mtriple=nvptx64-nvidia-cuda -mcpu=sm_80 r5_03.ll -o /dev/null
 #7 0x...  llvm::NVPTXAsmPrinter::bufferLEByte(llvm::Constant const*, int, llvm::NVPTXAsmPrinter::AggBuffer*)
 #8 0x...  llvm::NVPTXAsmPrinter::bufferAggregateConstant(llvm::Constant const*, llvm::NVPTXAsmPrinter::AggBuffer*)
 #9 0x...  llvm::NVPTXAsmPrinter::printModuleLevelGV(llvm::GlobalVariable const*, llvm::raw_ostream&, bool, llvm::NVPTXSubtarget const&)
#10 0x...  llvm::NVPTXAsmPrinter::emitGlobals(llvm::Module const&)
(exit code 134)

The struct case `{ i32, fp128 }` produces the identical crash via the same stack. The bare global `@f = addrspace(1) global fp128 ...` compiles cleanly (emits `.global .align 16 .b8 f[16] = {0,0,0,0,0,0,0,0,0,0,0,0,0,0,255,63};`, exit 0).
```

## Expected

llc should emit the 16-byte little-endian representation of each fp128 element, as it already does for a bare fp128 global and as x86 does for the nested cases. For `[2 x fp128] [1.0, 2.0]` it should emit a 32-byte initializer (the two fp128 values: low 8 bytes zero, high words 0x3FFF000000000000 and 0x4000000000000000 respectively), analogous to the x86 output:
  arr: .quad 0; .quad 0x3fff000000000000; .quad 0; .quad 0x4000000000000000  (32 bytes)
The fix is to add `case Type::FP128TyID:` alongside the other FP cases in bufferLEByte (line 1699), reusing the existing AddIntToBuffer(cast<ConstantFP>(CPV)->getValueAPF().bitcastToAPInt()) path, which already supports 128-bit widths.

## Verification

Independent verify + adversarial refute, both `confirmed` (verify confidence 0.98).

> Confirmed genuine llvm_unreachable crash on valid, non-UB IR.

Mechanism (verified against source at NVPTXAsmPrinter.cpp):
- bufferLEByte's floating-point switch (lines 1696-1701) handles only HalfTyID/BFloatTyID/FloatTyID/DoubleTyID. FP128TyID is absent, so an fp128 Constant falls through to `default: llvm_unreachable("unsupported type")` at line 1729.
- A bare top-level fp128 global works because printModuleLevelGV (lines 1066-1081) routes the initializer (a ConstantFP) directly into bufferAggregateConstant, which has a dedicated FP128 branch at lines 1759-1765 (ExtendBuffer of the 16-byte APInt). Confirmed: `@f = addrspace(1) global fp128 ...` emits `.b8 f[16] = {0,...,255,63}` and exits 0.
- But an fp128 nested in a ConstantArray (lines 1768-1772) or ConstantStruct (lines 1786-1799) is dispatched element-by-element via bufferLEByte, so each fp128 element hits the missing case -> unreachable.

Empirically reproduced with the provided llc on valid IR:
- `[2 x fp128]` array: crash, EXIT 134, "unsupported type / UNREACHABLE executed at ...NVPTXAsmPrinter.cpp:1729". Stack: bufferLEByte <- bufferAggregateConstant <- printModuleLevelGV <- emitGlobals.
- `{ i32, fp128 }` struct: identi
