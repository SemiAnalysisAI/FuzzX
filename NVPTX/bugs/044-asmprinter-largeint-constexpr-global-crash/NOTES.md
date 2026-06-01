# 044 — Large-integer (>64-bit) module-scope global with a ConstantExpr initializer crashes bufferAggregateConstant (llvm_unreachable "unsupported constant type")

- **Kind:** crash (abort/UB)
- **Reachable via:** default llc
- **Component:** NVPTXAsmPrinter.cpp 1733-1801 (unreachable at 1800); entered from printModuleLevelGV 1081  (round-7 area `C01-asmprinter-const-crash`)

## Summary

a >64-bit integer global with an unfolded ConstantExpr initializer crashes `bufferAggregateConstant` (`"unsupported constant type"`)

## Mechanism / root cause

A scalar integer global wider than 64 bits takes the IntegerTyID arm of printModuleLevelGV (line 1067, "Integers larger than 64 bits") and calls bufferAggregateConstant on the initializer. bufferAggregateConstant only recognizes ConstantInt (line 1752), ConstantFP-fp128 (1759), ConstantArray (1768), ConstantVector (1775), ConstantDataSequential (1780), ConstantStruct (1786); a ConstantExpr that the parser did NOT pre-fold (e.g. `bitcast (<3 x i32> ... to i96)`, `ptrtoint`, `add`) matches none and hits `llvm_unreachable("unsupported constant type in printAggregateConstant()")` at line 1800. Note bufferLEByte's IntegerTyID path *does* call ConstantFoldConstant on a ConstantExpr (line 1682), but bufferAggregateConstant does not, so this top-level large-int path is unprotected. Distinct from #014 (large int with a concrete APInt value, drops a partial high byte) and from #023/#029 (ptrtoint as an aggregate *element*).

## Trigger

A module-scope iN global (N>64) whose initializer is an unfolded ConstantExpr, e.g. `@g = global i96 bitcast (<3 x i32> <i32 1,i32 2,i32 3> to i96)`. Default llc, valid IR (verifier-clean; x86_64 emits the bytes correctly).

## Reproducer

```
target triple = "nvptx64-nvidia-cuda"
@g = global i96 bitcast (<3 x i32> <i32 1, i32 2, i32 3> to i96)
; also crashes: @g2 = global i128 bitcast (<2 x i64> <i64 1, i64 2> to i128)
```

Command: `llc -mtriple=nvptx64 -o - repro.ll`

## Verification

Reproduced with the built llc (crash/emitted-PTX matches the claim; finder confidence 0.88).
