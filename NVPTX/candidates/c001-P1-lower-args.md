# c001 — ArgUseChecker misclassifies unhandled pointer instructions (icmp/freeze/atomicrmw/cmpxchg) as read-only, crashing convertToParamAS with llvm_unreachable

- region: P1-lower-args
- file: NVPTXLowerArgs.cpp 309-383 (checker), 256 (unreachable), 437-445 (case 1)
- kind: segfault
- confidence(finder): 0.97

## Mechanism
ArgUseChecker (a PtrUseVisitor subclass) only overrides visitStoreInst, visitAddrSpaceCastInst, visitPtrToIntInst, visitPHINode, visitSelectInst, visitMemTransferInst, visitMemSetInst. Any other instruction that consumes the pointer (icmp, freeze, atomicrmw, cmpxchg, etc.) is not handled by PtrUseVisitor's base either, so dispatch falls to InstVisitor's default visitInstruction, which is a no-op: it sets neither PI.isEscaped() nor PI.isAborted(). visitArgPtr therefore returns a PtrInfo that is neither escaped nor aborted, so in lowerKernelByValParam (line 436) `ArgUseIsReadOnly = !(PI.isEscaped() || PI.isAborted())` becomes true, and with no phi/select the code enters case (1) at lines 437-445. There it calls convertToParamAS on every use of the arg, including the icmp/freeze/atomicrmw use. CloneInstInParamAS (lines 214-256) only matches LoadInst, GetElementPtrInst, BitCastInst, AddrSpaceCastInst, and MemTransferInst; for anything else it falls through to `llvm_unreachable("Unsupported instruction")` at line 256, aborting the compiler. Verified: `icmp eq ptr %p, null`, `freeze ptr %p`, and `atomicrmw add ptr %p` on a byval kernel param all crash at NVPTXLowerArgs.cpp:256 (both sm_60 and sm_90). The IR is well-defined and non-UB (e.g. comparing a byval pointer to null is simply false).

## Trigger
A kernel function (ptx_kernel calling convention) with a `byval` pointer argument where the argument pointer is directly used by an instruction outside the handled set: icmp, freeze, atomicrmw, or cmpxchg. Any -mcpu (sm_60, sm_90, etc.); the use must not also be a store/memcpy-dest/ptrtoint/non-param addrspacecast (those abort cleanly and take the copy path).

## IR
```
target triple = "nvptx64-nvidia-cuda"
%struct.S = type { i32, i32 }
define ptx_kernel void @kern(ptr byval(%struct.S) align 8 %p, ptr addrspace(1) %out) {
entry:
  %c = icmp eq ptr %p, null
  %z = zext i1 %c to i32
  store i32 %z, ptr addrspace(1) %out, align 4
  ret void
}
```

## llc cmd
`-mtriple=nvptx64 -mcpu=sm_90 -O2  (also crashes with -mcpu=sm_60; use -stop-after=nvptx-lower-args to isolate)`
