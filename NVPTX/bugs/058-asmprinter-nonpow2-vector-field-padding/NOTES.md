# 058 — Non-power-of-2 vector (store size < alloc size) as a non-last struct field / array element drops its tail padding, shifting all following fields left

- **Kind:** miscompile
- **Reachable via:** default llc
- **Component:** NVPTXAsmPrinter.cpp 1714-1726 (compensation), 1733-1799 (bufferAggregateConstant), 1768-1771 (array path)  (round-8 area `X02-asmprinter-struct-pad`)

## Summary

a non-power-of-2 vector (`<3 x i32>` etc.) as a non-last struct field/array element drops its tail padding, placing the following field at the wrong offset (e.g. trailing `i32` at byte 12 instead of 16)

## Mechanism / root cause

In bufferLEByte the aggregate case does:
  case FixedVectorTyID/StructTyID/ArrayTyID:
    if (isa<ConstantAggregate>||isa<ConstantDataSequential>) {
      bufferAggregateConstant(CPV, AggBuffer);          // (A)
      if (Bytes > AllocSize) AggBuffer->addZeros(Bytes - AllocSize); // (B)
    }
where AllocSize = DL.getTypeAllocSize(CPV->getType()). For a vector, bufferAggregateConstant (line 1780-1784, the ConstantDataSequential branch, and 1809-1812 for ConstantVector) emits exactly getNumElements()*elementStoreSize bytes = the vector's STORE size, NOT its ALLOC size. For non-power-of-2 vector lengths the two differ: <3 x i32> store=12 alloc=16, <3 x i16> store=6 alloc=8, <5 x i16> store=10 alloc=16, <6 x i8> store=6 alloc=8, <7 x i8> store=7 alloc=8, <3 x i64> store=24 alloc=32, etc.

When such a vector is a NON-LAST struct field, the parent (bufferAggregateConstant struct branch, lines 1789-1795) computes Bytes = elementOffset(I+1) - elementOffset(I) = the vector's ALLOC size (16 for <3 x i32>). So at (A) only 12 bytes are written; at (B) `Bytes(16) > AllocSize(16)` is FALSE, so the 4 missing padding bytes are NEVER added. The buffer cursor is left 4 bytes short and the next field is written at offset 12 instead of 16. When the vector is an ARRAY element (line 1768-1771) it is even worse: Bytes is passed as 0, so (B) is unconditionally skipped and every element after the first is shifted by the per-element padding.

Why wrong: the DataLayout puts the following field/element at the vector's ALLOC offset, but NVPTX emits it at the STORE offset. Cross-checked against x86 (which lays out per DataLayout correctly).

Fix would be to compare against the number of bytes actually written (the store size) rather than AllocSize, e.g. addZeros(Bytes - storeSize), and have the array path pass the per-element alloc size.

## Trigger

A module-scope (addrspace 0/1/4) global whose initializer is a struct or array containing a non-power-of-2-length vector (<3 x i32>, <3 x i16>, <5 x i16>, <6 x i8>, <7 x i8>, <3 x i64>, ...) in any position other than the very last leaf, with a non-zero field following it. Also reproduces through nested structs (e.g. struct{ struct{<3 x i32>}, i32 }) and packed structs (<{ <3 x i32>, i32 }>), and through arrays-of-vectors ([2 x <3 x i32>], where even the inter-element padding between consecutive vectors is dropped).

## Reproducer

```
target triple = "nvptx64-nvidia-cuda"
%s = type { <3 x i32>, i32 }
@g = global %s { <3 x i32> <i32 -1, i32 -1, i32 -1>, i32 -2 }

; also: @g_arr = global [2 x <3 x i32>] [<3 x i32> <i32 -1,i32 -1,i32 -1>, <3 x i32> <i32 -2,i32 -2,i32 -2>]
; also: %s16 = type { <3 x i16>, i16 } ; @g16 = global %s16 { <3 x i16> <i16 -1,i16 -1,i16 -1>, i16 -256 }
```

Command: `llc -mtriple=nvptx64-nvidia-cuda -o - repro.ll`

## Verification

Reproduced with the built llc (crash/emitted-PTX matches; finder confidence 0.95). For the blockaddress-as-zeros and non-pow2-vector cases the wrong layout was cross-checked against x86, which emits the correct bytes.
