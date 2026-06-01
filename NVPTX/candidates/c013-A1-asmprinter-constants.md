# c013 — Integer/FP vector splat path in bufferAggregateConstant ignores sub-byte packing (overflow/wrong layout for splat ConstantInt of sub-byte vector)

- region: A1-asmprinter-constants
- file: NVPTXAsmPrinter.cpp 1742-1749
- kind: segfault
- confidence(finder): 0.4

## Mechanism
When a vector constant is represented as a native splat ConstantInt/ConstantFP (i.e. `isa<ConstantInt,ConstantFP>(CPV)` is true and the type is a FixedVectorType), bufferAggregateConstant iterates VTy->getNumElements() and calls bufferLEByte on each scalar element (lines 1745-1746). For a sub-byte element type like <8 x i4>, getAggregateElement returns a scalar i4 ConstantInt for each lane, and bufferLEByte->AddIntToBuffer emits one byte per i4 (NumBytes=(4+7)/8=1), producing 8 bytes for a 4-byte vector. Unlike bufferAggregateConstVec, this path has no sub-byte packing, so it both produces a wrong (unpacked) layout and overflows the AggBuffer (addByte assert `curpos < Size` / OOB write in release). Confirmed crashing: `Assertion failed: (curpos < Size), function addByte`. NOTE: by default LLVM materializes fixed-length splats as ConstantVector (UseConstantIntForFixedLengthSplat defaults to false), so reaching this path through llc requires the hidden flag -use-constant-int-for-fixed-length-splat; hence lower confidence/severity, but the code path is latent and would also be hit by any frontend/transform that constructs splat ConstantInts directly for sub-byte vectors.

## Trigger
nvptx target plus the hidden flag -use-constant-int-for-fixed-length-splat (or IR constructed in-memory with a splat ConstantInt of a sub-byte vector type). A global initialized to a sub-byte vector splat, e.g. <8 x i4> splat (i4 3).

## IR
```
@g = addrspace(1) global <8 x i4> splat (i4 3)

```

## llc cmd
`-mtriple=nvptx64 -mcpu=sm_90 -O2 -use-constant-int-for-fixed-length-splat`
