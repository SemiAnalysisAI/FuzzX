# 057 — Scalar pointer global initialized to blockaddress crashes printScalarConstant (llvm_unreachable)

- **Kind:** crash (abort/UB)
- **Reachable via:** default llc
- **Component:** NVPTXAsmPrinter.cpp 1613-1642 (printScalarConstant), reached from printModuleLevelGV:1047  (round-8 area `X01-asmprinter-ptr-const`)

## Summary

a scalar `ptr`/`iN` global initialized to `blockaddress` (or `ptrtoint(blockaddress)`) crashes the AsmPrinter (`llvm_unreachable`)

## Mechanism / root cause

printScalarConstant() handles ConstantInt, ConstantFP, ConstantPointerNull, GlobalValue, and ConstantExpr, then falls through to `llvm_unreachable("Not scalar type found in printScalarConstant()")` at line 1642. A `BlockAddress` is a Constant but is none of those subclasses, so a module-scope scalar `ptr` global initialized to `blockaddress(@f, %bb)` reaches the unreachable. The block label IS emittable by NVPTX (it prints `$L__tmp0:` / `// Block address taken`), and x86 emits `.quad .Ltmp0` for the same IR, so this is valid input the backend partially supports — it just cannot emit the initializer. Crash, not a graceful diagnostic.

## Trigger

Module-scope scalar global of type `ptr` whose initializer is a `blockaddress` constant.

## Reproducer

```
define void @bar() {
entry:
  br label %lbl
lbl:
  ret void
}
@ba = global ptr blockaddress(@bar, %lbl)
```

Command: `llc -mtriple=nvptx64-nvidia-cuda -mcpu=sm_70 -o - repro.ll`

## Related sites (same unhandled-blockaddress root)
- scalar `ptr blockaddress` → `printScalarConstant` unreachable (1642)
- aggregate `ptrtoint(blockaddress)` element → `AggBuffer::printSymbol` unreachable (1151)
- scalar `ptrtoint(blockaddress)` → `lowerConstantForGV` unreachable (1898)

## Verification

Reproduced with the built llc (crash/emitted-PTX matches; finder confidence 0.97). 
