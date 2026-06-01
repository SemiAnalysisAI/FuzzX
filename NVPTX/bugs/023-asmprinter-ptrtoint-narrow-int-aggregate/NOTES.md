# 023 — ptrtoint to narrower-than-pointer int in aggregate initializer emits full pointer, dropping/overrunning following fields (miscompile + crash)

- **Kind:** miscompile / crash
- **Reachable via:** default llc
- **Component:** NVPTXAsmPrinter.cpp 1686-1691 (root cause in bufferLEByte PtrToInt case); manifests in printWords 1196-1215 and printBytes 1154-1194  (round-4 area `U06-asmprinter-global-init`)
- **Candidate id:** r4_00

## Summary

aggregate initializer with `ptrtoint(@sym to iN)` (N*8<ptrsize) emits a full 8-byte pointer, dropping the following field (or asserting)

## Mechanism / root cause

In bufferLEByte, the PtrToInt ConstantExpr case does:

  if (Cexpr->getOpcode() == Instruction::PtrToInt) {
    Value *V = Cexpr->getOperand(0)->stripPointerCasts();
    AggBuffer->addSymbol(V, Cexpr->getOperand(0));
    AggBuffer->addZeros(AllocSize);   // AllocSize = sizeof(int result type)
    break;
  }

It records a symbol at curpos and reserves only AllocSize bytes, where AllocSize is the size of the *integer* result type (e.g. 4 for i32). But both printWords and printBytes unconditionally emit a full ptrSize (8 on nvptx64) bytes per symbol: printWords advances `pos += ptrSize` per symbol and printBytes emits exactly ptrSize mask() bytes (`for (i=0; i<ptrSize; ++i) ... 0xFF<<i*8(sym)`). When AllocSize < ptrSize the symbol's emitted bytes consume the next field(s).

For a word-aligned aggregate (ElementSize % ptrSize == 0, all symbols ptr-aligned) the printWords path is taken. With `{i32 ptrtoint(@g to i32), i32 0x12345678}` (Size=8, ptrSize=8): symbolPosInBuffer=[0]; printWords pos=0 emits `g` then sets nextSymbolPos=Size=8, loop ends -> only `g` emitted as a single u64. The 0x12345678 second field sitting in buffer[4..8] is never read. Result `s[1] = {g}`: a full 64-bit pointer where the IR is the low 32 address bits followed by a distinct 32-bit constant. The second field is silently dropped and the first field is widened from 32 to 64 bits -> wrong bytes.

In the unaligned (printBytes) variant the position bookkeeping desyncs: after emitting ptrSize mask bytes `pos` overshoots the recorded position of the next field, violating `assert(nextSymbolPos >= pos)` (crash). The correct behavior (per x86) is to truncate the pointer to the int width and emit only AllocSize bytes for it.

## Trigger

A global aggregate (struct/array) initializer in addrspace(1)/addrspace(4) containing `ptrtoint(@sym to iN)` where N*8 < pointer-size-in-bits (e.g. i32 under default 64-bit nvptx64 pointers), with at least one following field. Word-aligned layout -> silent miscompile; non-pointer-aligned/packed layout -> assertion crash (or report_fatal_error on PTX<7.1).

## Reproducer

```
target triple = "nvptx64-nvidia-cuda"
@g = addrspace(1) global i32 0
@s = addrspace(1) global { i32, i32 } { i32 ptrtoint (ptr addrspace(1) @g to i32), i32 305419896 }
```

Command: `llc -mtriple=nvptx64 -mcpu=sm_70 -o - repro.ll`

## Observed (wrong) output

```
.visible .global .align 4 .u32 g;
.visible .global .align 8 .u64 s[1] = {g};

(assertion-crash variant, llc -mtriple=nvptx64 -mcpu=sm_90 r4_00_crash.ll with @s2 = <{ i32 ptrtoint(@g to i32), ptr addrspace(1) @h }>:)
Assertion failed: (nextSymbolPos >= pos), function printBytes, file NVPTXAsmPrinter.cpp, line 1192.
... (SIGABRT, exit code 134)
```

## Expected

@s is a struct of two independent 4-byte fields: field0 = low 32 bits of &g, field1 = 0x12345678. A correct emission keeps both as 4-byte values, e.g. (x86 cross-check) ".long g / .long 305419896", and (correct 32-bit nvptx output) ".u32 s[2] = {g, 305419896}". On nvptx64 it should emit the low 32 bits of the symbol for the i32 field (truncated to 4 bytes via a mask/AllocSize) and then the second field 305419896 — NOT a single 8-byte .u64 that swallows the constant and widens the address to 64 bits.

## Verification

Independent verify + adversarial refute confirmed 

> Confirmed both the miscompile and the assertion-crash variant on valid, non-UB IR.

Root cause (NVPTXAsmPrinter.cpp:1686-1691, bufferLEByte PtrToInt case):
  Value *V = Cexpr->getOperand(0)->stripPointerCasts();
  AggBuffer->addSymbol(V, Cexpr->getOperand(0));
  AggBuffer->addZeros(AllocSize);   // AllocSize = getTypeAllocSize(result int) = 4 for i32
addSymbol records the symbol at curpos and addZeros advances curpos by only AllocSize (4). But both printWords and printBytes emit a FULL ptrSize (8 on nvptx64) per symbol. The mismatch (AllocSize 4 < ptrSize 8) is the bug. The PtrToInt branch is genuinely reached: ConstantFoldConstant cannot fold ptrtoint(@g) since the address is symbolic (confirmed: opt -verify leaves the ConstantExpr intact, exit 0).

MISCOMPILE (word-aligned path, printWords): For @s = {i32 ptrtoint(@g to i32), i32 305419896}, Size=8, ElementSize%ptrSize==0 and the symbol is at offset 0 (aligned), so printWords runs. It pushes symbolPosInBuffer=[0, Size=8]; pos=0 matches nextSymbolPos=0 so it emits printSymbol(g), sets nextSymbolPos=8, then pos+=ptrSize=8 ends the loop. Result: .u64 s[1] = {g}. The second field 305419896 (0x12345678), which lives in buffer[4..8], i
