# 042 — Top-level x86_fp80/ppc_fp128 module-scope global crashes printModuleLevelGV (llvm_unreachable "type not supported yet")

- **Kind:** crash (abort/UB)
- **Reachable via:** default llc
- **Component:** NVPTXAsmPrinter.cpp 1066-1127 (unreachable at 1126)  (round-7 area `C01-asmprinter-const-crash`)

## Summary

a top-level `x86_fp80`/`ppc_fp128` global crashes `printModuleLevelGV` (`"type not supported yet"`)

## Mechanism / root cause

The aggregate/large-scalar branch of printModuleLevelGV switches on ETy->getTypeID() and only enumerates IntegerTyID, FP128TyID, StructTyID, ArrayTyID, FixedVectorTyID (lines 1067-1071); the default arm is `llvm_unreachable("type not supported yet")` at 1126. x86_fp80 (10 bytes, alloc 16) and ppc_fp128 (16 bytes) are floating types >64 bits so they do NOT take the scalar branch at line 1027 (scalarSizeInBits>64), and their TypeIDs (X86_FP80TyID / PPC_FP128TyID) are not in the switch, so a top-level global of either type crashes here. README #030 explicitly states top-level fp128 is fine and does not cover x86_fp80/ppc_fp128, which crash at a different line (1126) than #030 (1729).

## Trigger

A module-scope global of type x86_fp80 or ppc_fp128 with a non-zero initializer, e.g. `@g = global x86_fp80 0xK4000C90FDAA22168C235`. Default llc, no flags.

## Reproducer

```
target triple = "nvptx64-nvidia-cuda"
@g = global x86_fp80 0xK4000C90FDAA22168C235
@p = global ppc_fp128 0xM40090000000000000000000000000000
```

Command: `llc -mtriple=nvptx64 -o - repro.ll`

## Verification

Reproduced with the built llc (crash/emitted-PTX matches the claim; finder confidence 0.9).
