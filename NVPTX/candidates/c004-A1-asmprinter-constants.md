# c004 — bufferAggregateConstVec compares whole-global buffer size against vector element count, mislaying/overflowing nested sub-byte vectors

- region: A1-asmprinter-constants
- file: NVPTXAsmPrinter.cpp 1803-1813
- kind: segfault
- confidence(finder): 0.92

## Mechanism
bufferAggregateConstVec decides whether to do sub-byte packing by comparing `BuffSize = aggBuffer->getBufferSize()` against `NumElems = CV->getType()->getNumElements()`. But `BuffSize` is the size of the ENTIRE top-level global, not of the current vector. The AggBuffer is constructed once in printModuleLevelGV (line 1080: `AggBuffer aggBuffer(ElementSize, *this)`) with `ElementSize = DL.getTypeStoreSize(ETy)` of the whole global, and is threaded recursively into every sub-element. For a sub-byte-element vector (e.g. <8 x i4>, alloc size 4 bytes) that is NESTED inside a larger aggregate, BuffSize is the whole struct/array size, so `BuffSize >= NumElems` is true and the code takes the 'one element at a time' branch at lines 1809-1811. That calls bufferLEByte on each i4 scalar; bufferLEByte->AddIntToBuffer computes NumBytes=(4+7)/8=1 and emits ONE BYTE PER i4 element. So an <8 x i4> field writes 8 bytes instead of the packed 4 bytes (and <2 x i4> writes 2 bytes instead of 1). This (a) lays out the field at double width, shifting every following field to the wrong offset (a layout miscompile), and (b) makes total bytes written = correctSize + extra, which always exceeds the buffer's Size, so AggBuffer::addByte (NVPTXAsmPrinter.h:124-128, `buffer[curpos]` with `assert(curpos < Size)`) overflows the std::vector. In an assertions build this fires `Assertion failed: (curpos < Size)`; in a release build it is an out-of-bounds heap write. The correct comparison should use the current vector's own alloc size (e.g. DL.getTypeAllocSize(CV->getType())) rather than the global buffer size. A standalone top-level <8 x i4> works correctly only because there BuffSize coincidentally equals the vector's alloc size (4) which is < NumElems (8), triggering the packing path.

## Trigger
Any nvptx target. A global (or const/global addrspace) initializer whose aggregate contains a vector with sub-byte integer elements (i4) where the element type is not ConstantDataVector-compatible, so it materializes as a ConstantVector, nested inside a struct/array so the top-level buffer size exceeds the vector's element count. e.g. { <8 x i4>, i32 } or [2 x <8 x i4>] or { <2 x i4>, [16 x i8] }. No special flags needed; -O0 or -O2 both hit it.

## IR
```
@g = addrspace(1) global { <8 x i4>, i32 } { <8 x i4> <i4 1, i4 2, i4 3, i4 4, i4 5, i4 6, i4 7, i4 8>, i32 305419896 }

```

## llc cmd
`-mtriple=nvptx64 -mcpu=sm_90 -O2`
