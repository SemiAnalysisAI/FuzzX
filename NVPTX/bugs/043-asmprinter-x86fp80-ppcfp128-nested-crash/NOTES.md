# 043 — x86_fp80/ppc_fp128 element nested in array/struct global crashes bufferLEByte (llvm_unreachable "unsupported type")

- **Kind:** crash (abort/UB)
- **Reachable via:** default llc
- **Component:** NVPTXAsmPrinter.cpp 1674-1730 (unreachable at 1729); bufferAggregateConstant 1733-1801 has no handler for these types  (round-7 area `C01-asmprinter-const-crash`)

## Summary

an `x86_fp80`/`ppc_fp128` element nested in an array/struct global crashes `bufferLEByte` (`"unsupported type"`)

## Mechanism / root cause

bufferLEByte's type switch only handles Half/BFloat/Float/Double under the FP cases (1696-1700); x86_fp80 and ppc_fp128 fall to `default: llvm_unreachable("unsupported type")` at 1729. bufferAggregateConstant has a dedicated branch ONLY for isFP128Ty() (line 1761) - it has no x86_fp80/ppc_fp128 path - so when one of these appears as an array/struct element, the per-element bufferLEByte call crashes. Same crash *line* as README #030, but #030 is fp128-specific and its fix (the line-1761 FP128 branch) does not cover x86_fp80/ppc_fp128, so these are distinct unhandled types.

## Trigger

An x86_fp80 or ppc_fp128 element inside an array or struct global initializer, e.g. `@arr = global [2 x x86_fp80] [...]` or `%s=type{i32,fp80}`. Default llc.

## Reproducer

```
target triple = "nvptx64-nvidia-cuda"
@arr = global [2 x x86_fp80] [x86_fp80 0xK4000C90FDAA22168C235, x86_fp80 0xK3FFF8000000000000000]
; also: @parr = global [2 x ppc_fp128] [ppc_fp128 0xM40090000000000000000000000000000, ppc_fp128 0xM3FF00000000000000000000000000000]
```

Command: `llc -mtriple=nvptx64 -o - repro.ll`

## Verification

Reproduced with the built llc (crash/emitted-PTX matches the claim; finder confidence 0.82).
