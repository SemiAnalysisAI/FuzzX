# 010 — bufferAggregateConstVec compares whole-global buffer size against vector element count, mislaying/overflowing nested sub-byte vectors

- **Kind:** crash (OOB write)
- **Reachable via:** default llc
- **Component:** NVPTXAsmPrinter.cpp 1803-1813  (region `A1-asmprinter-constants`)
- **Candidate id:** c004

## Summary

global with a nested sub-byte vector (`<8 x i4>`) overflows the AsmPrinter constant buffer

## Mechanism / root cause

bufferAggregateConstVec decides whether to do sub-byte packing by comparing `BuffSize = aggBuffer->getBufferSize()` against `NumElems = CV->getType()->getNumElements()`. But `BuffSize` is the size of the ENTIRE top-level global, not of the current vector. The AggBuffer is constructed once in printModuleLevelGV (line 1080: `AggBuffer aggBuffer(ElementSize, *this)`) with `ElementSize = DL.getTypeStoreSize(ETy)` of the whole global, and is threaded recursively into every sub-element. For a sub-byte-element vector (e.g. <8 x i4>, alloc size 4 bytes) that is NESTED inside a larger aggregate, BuffSize is the whole struct/array size, so `BuffSize >= NumElems` is true and the code takes the 'one element at a time' branch at lines 1809-1811. That calls bufferLEByte on each i4 scalar; bufferLEByte->AddIntToBuffer computes NumBytes=(4+7)/8=1 and emits ONE BYTE PER i4 element. So an <8 x i4> field writes 8 bytes instead of the packed 4 bytes (and <2 x i4> writes 2 bytes instead of 1). This (a) lays out the field at double width, shifting every following field to the wrong offset (a layout miscompile), and (b) makes total bytes written = correctSize + extra, which always exceeds the buffer's Size, so AggBuffer::addByte (NVPTXAsmPrinter.h:124-128, `buffer[curpos]` with `assert(curpos < Size)`) overflows the std::vector. In an assertions build this fires `Assertion failed: (curpos < Size)`; in a release build it is an out-of-bounds heap write. The correct comparison should use the current vector's own alloc size (e.g. DL.getTypeAllocSize(CV->getType())) rather than the global buffer size. A standalone top-level <8 x i4> works correctly only because there BuffSize coincidentally equals the vector's alloc size (4) which is < NumElems (8), triggering the packing path.

## Trigger

Any nvptx target. A global (or const/global addrspace) initializer whose aggregate contains a vector with sub-byte integer elements (i4) where the element type is not ConstantDataVector-compatible, so it materializes as a ConstantVector, nested inside a struct/array so the top-level buffer size exceeds the vector's element count. e.g. { <8 x i4>, i32 } or [2 x <8 x i4>] or { <2 x i4>, [16 x i8] }. No special flags needed; -O0 or -O2 both hit it.

## Reproducer

See `repro.ll` / `cmd.sh`.

```
@g = addrspace(1) global { <8 x i4>, i32 } { <8 x i4> <i4 1, i4 2, i4 3, i4 4, i4 5, i4 6, i4 7, i4 8>, i32 305419896 }
```

Command:

```
llc -mtriple=nvptx64 -mcpu=sm_90 -O2 -o - repro.ll
```

## Observed (wrong) output

```
Assertion failed: (curpos < Size), function addByte, file NVPTXAsmPrinter.h, line 125.
PLEASE submit a bug report to https://github.com/llvm/llvm-project/issues/ and include the crash backtrace and instructions to reproduce the bug.
Stack dump:
0.	Program arguments: /Users/justinlebar/code/llvm2/build/bin/llc -mtriple=nvptx64 -mcpu=sm_90 -O2 /Users/justinlebar/code/FuzzX/NVPTX/scratch/c004.ll -o -
 #7 llvm::NVPTXAsmPrinter::AggBuffer::addBytes(unsigned char const*, unsigned int, unsigned int)
 #8 llvm::NVPTXAsmPrinter::bufferLEByte(llvm::Constant const*, int, llvm::NVPTXAsmPrinter::AggBuffer*)::$_0::operator()(llvm::APInt const&) const
 #9 llvm::NVPTXAsmPrinter::bufferAggregateConstant(llvm::Constant const*, llvm::NVPTXAsmPrinter::AggBuffer*)
#10 llvm::NVPTXAsmPrinter::printModuleLevelGV(...)
#11 llvm::NVPTXAsmPrinter::emitGlobals(llvm::Module const&)
(exit code 134, SIGABRT)
```

## Expected

No crash. The nested <8 x i4> should be packed into 4 bytes (each byte holding two i4 elements little-endian: 1|(2<<4)=0x21=33, 3|(4<<4)=0x43=67, 5|(6<<4)=0x65=101, 7|(8<<4)=0x87=135), followed by the little-endian i32 305419896=0x12345678 -> {120,86,52,18}, e.g. `.global .align 8 .b8 g[8] = {33, 67, 101, 135, 120, 86, 52, 18};`. (Confirmed by the working standalone top-level case `@g = addrspace(1) global <8 x i4> ...` which correctly emits `.global .align 4 .b8 g[4] = {33, 67, 101, 135};`.)

## Verification

Verified empirically with the built llc (independent verify + adversarial
refute both `confirmed`, finder confidence 0.92, verify confidence 0.98).

> Confirmed real. Source (NVPTXAsmPrinter.cpp:1803-1813) shows bufferAggregateConstVec decides whether to do sub-byte packing via `if (BuffSize >= NumElems)` where BuffSize = aggBuffer->getBufferSize() returns AggBuffer::Size (NVPTXAsmPrinter.h:113), which is the ElementSize of the ENTIRE top-level global (set once at NVPTXAsmPrinter.cpp:1080: `AggBuffer aggBuffer(ElementSize, *this)` with ElementSize=DL.getTypeStoreSize(ETy) of the whole global) and is threaded recursively into nested elements. NumElems is the CURRENT vector's element count. For a sub-byte-element ConstantVector (i4, which is not ConstantDataVector-compatible so it materializes as ConstantVector and routes to line 1775->bufferAggregateConstVec) nested inside a larger aggregate, BuffSize is the whole struct/array size, so BuffSize>=NumElems is true and the buggy "one element at a time" branch (1809-1811) runs: it calls bufferLEByte on each i4 scalar; AddIntToBuffer (1657-1672) computes NumBytes=(4+7)/8=1 and emits ONE BYTE per i4 element. So <8 x i4> writes 8 bytes instead of the packed 4. Total bytes written exceeds the buffer Size, so AggBuffer::addByte (NVPTXAsmPrinter.h:124-128, `assert(curpos < Size); buffer[curpos]=...`) overflows. EMPIRICALLY VERIFIED: with the candidate IR { <8 x i4>, i32 } (struct alloc size 8: vector 4 bytes + i32 4 bytes), llc asserts `Assertion failed: (curpos < Size), function addByte, file NVPTXAsmPrinter.h, line 125` with a stack trace through bufferAggregateConstant->bufferLEByt
