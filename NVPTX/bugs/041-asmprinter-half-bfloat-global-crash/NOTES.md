# 041 — Top-level half/bfloat module-scope global crashes printFPConstant (llvm_unreachable "unsupported fp type")

- **Kind:** crash (abort/UB)
- **Reachable via:** default llc
- **Component:** NVPTXAsmPrinter.cpp 1591-1611 (unreachable at 1607); reached via printScalarConstant 1619 from printModuleLevelGV 1047  (round-7 area `C01-asmprinter-const-crash`)

## Summary

a module-scope scalar `half`/`bfloat` global crashes `printFPConstant` (`llvm_unreachable "unsupported fp type"`) — these are common CUDA types

## Mechanism / root cause

printFPConstant only handles Type::FloatTyID and Type::DoubleTyID; every other FP type falls to `llvm_unreachable("unsupported fp type")` at line 1607. A scalar `half`/`bfloat` global has scalarSizeInBits<=64, so printModuleLevelGV takes the scalar branch (line 1027-1035) and calls printScalarConstant -> printFPConstant on the ConstantFP, which crashes. (Half/bfloat *array elements* are fine because they go through bufferLEByte's HalfTyID/BFloatTyID case at lines 1696-1697; only the top-level scalar path is broken.) The README's #030 only concerns fp128; it explicitly notes top-level fp128 is fine and says nothing about half/bfloat, which are far more common in real CUDA code.

## Trigger

A module-scope (global or const addrspace) scalar `half` or `bfloat` global with any non-zero, non-undef initializer, e.g. `@h = global half 0xH3C00`. Reproduces on default `llc` (sm_75/PTX 6.3), no flags.

## Reproducer

```
target triple = "nvptx64-nvidia-cuda"
@h = global half 0xH3C00
@b = global bfloat 0xR3F80
```

Command: `llc -mtriple=nvptx64 -o - repro.ll`

## Verification

Reproduced with the built llc (crash/emitted-PTX matches the claim; finder confidence 0.97).
