# 060 — NVPTXReplaceImageHandles: nvcl tex/surf handle loaded from a non-symbol address asserts / reads a non-symbol MachineOperand (release OOB)

- **Kind:** crash (assert/UB)
- **Reachable via:** llc nvptx64-unknown-nvcl
- **Component:** NVPTXReplaceImageHandles.cpp 1803-1817 (assert at 1811, getSymbolName at 1812)  (round-8 area `X15-tex-surf2`)

## Summary

on an nvcl target, a tex/surf handle loaded from a non-symbol (register/alloca) address asserts in NVPTXReplaceImageHandles (`isSymbol()`)

## Mechanism / root cause

In the LD_i64 branch of replaceImageHandle, after the CUDA early-return, the code assumes the handle's defining load is a load of a kernel-parameter symbol:

  assert(TexHandleDef.getOperand(7).isSymbol() && "Load is not a symbol!");
  StringRef Sym = TexHandleDef.getOperand(7).getSymbolName();

The LD instruction's operand layout (NVPTXInstrInfo.td:1939-1947) is (dst, sem, scope, addsp, Sign, fromWidth, usedBytes, ADDR), where ADDR is a complex operand expanding to (ADDR_base, i32imm). So operand 7 is the load's base. For a normal nvcl surfref/texref kernel param, the base is an ExternalSymbol and the assert holds. But if the i64 handle is produced by an ordinary memory load whose base is a *register* (or frameindex/alloca) rather than a symbol -- e.g. `%img = load i64, ptr %handleptr` -- operand 7 is a register MachineOperand. The assert (`isSymbol()`) fires in assertions builds; in a release (NDEBUG) build the assert is gone and getOperand(7).getSymbolName() reads the symbol-name field of a register/immediate operand union -> garbage StringRef -> Op.ChangeToES(garbage) emits a bogus .param symbol name or crashes. Valid IR (loading a surfref handle through a generic pointer) thus aborts/UB. Distinct from catalog #018 (select->SELP, default-case unreachable) and from the PHI/call default-case crash: here the def *is* an LD_i64, so it takes this branch and the operand-7-is-symbol assert (line 1811) fires, not the line-1835 unreachable. The CUDA path returns false at line 1807-1809 and does not crash, so this is nvcl/OpenCL-target specific.

## Trigger

Target nvptx64-unknown-nvcl (or any non-CUDA NVPTX driver interface), sm_20+; a tex/surf intrinsic whose i64 image handle is produced by a load from a register/alloca pointer instead of from a kernel-parameter symbol.

## Reproducer

```
target triple = "nvptx64-unknown-nvcl"

declare i32 @llvm.nvvm.suld.1d.i32.trap(i64, i32)

define ptx_kernel void @foo(ptr %handleptr, ptr %red, i32 %idx) {
  %img = load i64, ptr %handleptr
  %val = tail call i32 @llvm.nvvm.suld.1d.i32.trap(i64 %img, i32 %idx)
  store i32 %val, ptr %red
  ret void
}

!nvvm.annotations = !{!1}
!1 = !{ptr @foo, !"rdwrimage", i32 0}
```

Command: `llc -mcpu=sm_20 -o - repro.ll`

## Verification

Reproduced with the built llc (crash/emitted-PTX matches; finder confidence 0.9). 
