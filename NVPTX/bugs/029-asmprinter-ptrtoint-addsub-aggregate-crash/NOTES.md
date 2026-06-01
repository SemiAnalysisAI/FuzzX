# 029 — bufferLEByte crashes (llvm_unreachable) on integer aggregate element that is symbol+offset (add/sub of ptrtoint)

- **Kind:** crash (abort/UB)
- **Reachable via:** default llc
- **Component:** NVPTXAsmPrinter.cpp 1680-1693  (round-5 area `V11-asmprinter-const-more`)
- **Candidate id:** r5_02

## Summary

aggregate initializer element `add/sub(ptrtoint(@sym), C)` hits `llvm_unreachable("unsupported integer const type")` in bufferLEByte

## Mechanism / root cause

In bufferLEByte's IntegerTyID case, a ConstantExpr element is handled only if (a) ConstantFoldConstant folds it to a ConstantInt, or (b) its top-level opcode is exactly Instruction::PtrToInt (lines 1686-1691). A symbol-relative integer whose offset is applied OUTSIDE the ptrtoint -- e.g. `add (i64 ptrtoint(@g), i64 16)` or `sub (i64 ptrtoint(@g), i64 8)` -- has top-level opcode Add/Sub, contains a symbol so cannot be folded to a ConstantInt, and is NOT PtrToInt. It falls through to `llvm_unreachable("unsupported integer const type")` at line 1693. The same constant works (i) at top-level scalar globals (different path: prints `x = g+16`) and (ii) when the offset is inside the ptrtoint as a GEP (`ptrtoint(gep(@g,+2))` prints `{g+8}`), proving the backend intends to support symbol+offset integers; only the add/sub-outside-ptrtoint form in an aggregate is missed. x86 reference emits `g+16` / `g-8` correctly.

## Trigger

A global array/struct whose integer-typed element is `add(ptrtoint(@sym), C)` or `sub(ptrtoint(@sym), C)`. Reproduced: `@arr = addrspace(1) global [2 x i64] [i64 ptrtoint(ptr addrspace(1) @g to i64), i64 add(i64 ptrtoint(ptr addrspace(1) @g to i64), i64 16)]` crashes; the sub form crashes too.

## Reproducer

```
target triple = "nvptx64-nvidia-cuda"
@g = addrspace(1) global i32 42
@arr = addrspace(1) global [2 x i64] [i64 ptrtoint (ptr addrspace(1) @g to i64), i64 add (i64 ptrtoint (ptr addrspace(1) @g to i64), i64 16)]
```

Command: `llc -mtriple=nvptx64 -mcpu=sm_90 -o - repro.ll`

## Observed (wrong) output

```
unsupported integer const type
UNREACHABLE executed at /Users/justinlebar/code/llvm2/llvm/lib/Target/NVPTX/NVPTXAsmPrinter.cpp:1693!
PLEASE submit a bug report to https://github.com/llvm/llvm-project/issues/ and include the crash backtrace and instructions to reproduce the bug.
Stack dump:
0.	Program arguments: /Users/justinlebar/code/llvm2/build/bin/llc -mtriple=nvptx64-nvidia-cuda -mcpu=sm_80 /Users/justinlebar/code/FuzzX/NVPTX/scratch/r5_02.ll -o -
 #7  llvm::NVPTXAsmPrinter::bufferLEByte(llvm::Constant const*, int, llvm::NVPTXAsmPrinter::AggBuffer*)
 #8  llvm::NVPTXAsmPrinter::bufferAggregateConstant(llvm::Constant const*, llvm::NVPTXAsmPrinter::AggBuffer*)
 #9  llvm::NVPTXAsmPrinter::printModuleLevelGV(...)
#10  llvm::NVPTXAsmPrinter::emitGlobals(llvm::Module const&)
#11  llvm::NVPTXAsmPrinter::doFinalization(llvm::Module&)

(The sub form, i64 sub(i64 ptrtoint(@g), i64 8), crashes identically.)
```

## Expected

llc should emit the global without crashing, encoding the symbol-relative integer element as symbol+offset, analogous to x86 and to NVPTX's own scalar/GEP paths. Concretely NVPTX should produce something like:
  .visible .global .align 8 .u64 arr[2] = {g, g+16};
For reference, x86_64 emits:
  arr:
    .quad g
    .quad g+16
NVPTX already emits "x = g+16" for the equivalent scalar global and "arr[2] = {g, g+8}" for the GEP-inside-ptrtoint aggregate form, so the add/sub-outside-ptrtoint aggregate case should be handled the same way instead of hitting llvm_unreachable.

## Verification

Independent verify + adversarial refute, both `confirmed` (verify confidence 0.98).

> Confirmed real assertion/unreachable crash on valid IR. Mechanism verified by reading NVPTXAsmPrinter.cpp:1674-1694: in bufferLEByte's Type::IntegerTyID case, a ConstantExpr element is only handled if (a) ConstantFoldConstant folds it to a ConstantInt, or (b) its top-level opcode is exactly Instruction::PtrToInt. A symbol-relative integer whose offset is applied OUTSIDE the ptrtoint -- add(ptrtoint(@g),16) or sub(ptrtoint(@g),8) -- has top-level opcode Add/Sub, contains a symbol so cannot fold to a ConstantInt, and is not PtrToInt, so it falls through to llvm_unreachable("unsupported integer const type") at line 1693.

Empirical verification with /Users/justinlebar/code/llvm2/build/bin/llc:
1. The repro IR passes opt -passes=verify (valid, non-UB).
2. llc crashes with "unsupported integer const type / UNREACHABLE executed at .../NVPTXAsmPrinter.cpp:1693". Stack trace confirms: NVPTXAsmPrinter::bufferLEByte -> bufferAggregateConstant -> printModuleLevelGV -> emitGlobals -> doFinalization. The add and sub forms both crash.
3. x86 reference (x86_64-unknown-linux-gnu) emits the same global cleanly: arr: .quad g ; .quad g+16. This is well-defined IR a correct backend handles.
4. The NVP
