# 008 — ArgUseChecker misclassifies unhandled pointer instructions (icmp/freeze/atomicrmw/cmpxchg) as read-only, crashing convertToParamAS with llvm_unreachable

- **Kind:** crash (abort/UB)
- **Reachable via:** default llc
- **Component:** NVPTXLowerArgs.cpp 309-383 (checker), 256 (unreachable), 437-445 (case 1)  (region `P1-lower-args`)
- **Candidate id:** c001

## Summary

byval kernel param used by icmp/freeze/atomicrmw/cmpxchg hits `llvm_unreachable` in NVPTXLowerArgs

## Mechanism / root cause

ArgUseChecker (a PtrUseVisitor subclass) only overrides visitStoreInst, visitAddrSpaceCastInst, visitPtrToIntInst, visitPHINode, visitSelectInst, visitMemTransferInst, visitMemSetInst. Any other instruction that consumes the pointer (icmp, freeze, atomicrmw, cmpxchg, etc.) is not handled by PtrUseVisitor's base either, so dispatch falls to InstVisitor's default visitInstruction, which is a no-op: it sets neither PI.isEscaped() nor PI.isAborted(). visitArgPtr therefore returns a PtrInfo that is neither escaped nor aborted, so in lowerKernelByValParam (line 436) `ArgUseIsReadOnly = !(PI.isEscaped() || PI.isAborted())` becomes true, and with no phi/select the code enters case (1) at lines 437-445. There it calls convertToParamAS on every use of the arg, including the icmp/freeze/atomicrmw use. CloneInstInParamAS (lines 214-256) only matches LoadInst, GetElementPtrInst, BitCastInst, AddrSpaceCastInst, and MemTransferInst; for anything else it falls through to `llvm_unreachable("Unsupported instruction")` at line 256, aborting the compiler. Verified: `icmp eq ptr %p, null`, `freeze ptr %p`, and `atomicrmw add ptr %p` on a byval kernel param all crash at NVPTXLowerArgs.cpp:256 (both sm_60 and sm_90). The IR is well-defined and non-UB (e.g. comparing a byval pointer to null is simply false).

## Trigger

A kernel function (ptx_kernel calling convention) with a `byval` pointer argument where the argument pointer is directly used by an instruction outside the handled set: icmp, freeze, atomicrmw, or cmpxchg. Any -mcpu (sm_60, sm_90, etc.); the use must not also be a store/memcpy-dest/ptrtoint/non-param addrspacecast (those abort cleanly and take the copy path).

## Reproducer

See `repro.ll` / `cmd.sh`.

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

Command:

```
llc -mtriple=nvptx64 -mcpu=sm_90 -O2 -o - repro.ll
```

## Observed (wrong) output

```
Unsupported instruction
UNREACHABLE executed at /Users/justinlebar/code/llvm2/llvm/lib/Target/NVPTX/NVPTXLowerArgs.cpp:256!
PLEASE submit a bug report to https://github.com/llvm/llvm-project/issues/ and include the crash backtrace and instructions to reproduce the bug.
Stack dump:
0.	Program arguments: /Users/justinlebar/code/llvm2/build/bin/llc -mtriple=nvptx64 -mcpu=sm_90 -O2 /Users/justinlebar/code/FuzzX/NVPTX/scratch/c001.ll -o -
1.	Running pass 'Function Pass Manager' on module '...c001.ll'.
2.	Running pass 'Lower pointer arguments of CUDA kernels' on function '@kern'
 #7 processFunction(llvm::Function&, llvm::NVPTXTargetMachine&)
(also crashes identically with -mcpu=sm_60, with -stop-after=nvptx-lower-args, and for `freeze ptr %p` and `atomicrmw add ptr %p, i32 1 seq_cst`)
```

## Expected

llc should compile this valid IR without crashing. The ArgUseChecker should treat icmp/freeze/atomicrmw/cmpxchg (any pointer-consuming instruction it cannot convert to param AS) as a use that requires the safe copy path — i.e. PI.setAborted(...) for them (mirroring how visitMemSetInst / non-param addrspacecast abort) — so lowerKernelByValParam falls out of case (1) and instead creates a local copy via copyByValParam. Correct PTX would materialize a generic-AS copy of the byval struct (alloca + ld.param/memcpy) and then perform the icmp against it; for this input the comparison is well-defined false, so %out receives 0.

## Verification

Verified empirically with the built llc (independent verify + adversarial
refute both `confirmed`, finder confidence 0.97, verify confidence 0.98).

> Confirmed real crash on valid, non-UB IR. Mechanism verified against source:

1. ArgUseChecker (NVPTXLowerArgs.cpp:309-383), a PtrUseVisitor subclass, overrides only visitStoreInst, visitAddrSpaceCastInst, visitPtrToIntInst, visitPHINode, visitSelectInst, visitMemTransferInst, visitMemSetInst. PtrUseVisitor's base (PtrUseVisitor.h) additionally handles bitcast/GEP/intrinsic/callbase but NOT icmp/freeze/atomicrmw/cmpxchg. For those, dispatch falls to InstVisitor::visitInstruction (a no-op) which sets neither PI.isEscaped() nor PI.isAborted().

2. visitArgPtr (line 331) only breaks the worklist loop on PI.isAborted(), so the visit completes with PI clean. At line 436, ArgUseIsReadOnly = !(isEscaped()||isAborted()) becomes true, and with no phi/select (Conditionals empty) the code enters case (1) at lines 437-445, calling convertToParamAS on every use of the arg including the icmp use.

3. convertToParamAS's CloneInstInParamAS (lines 214-256) matches only LoadInst/GetElementPtrInst/BitCastInst/AddrSpaceCastInst/MemTransferInst; for icmp it falls through to llvm_unreachable("Unsupported instruction") at line 256, aborting the compiler.

Empirically reproduced with the built llc (Optimized build with assertions): icmp, freeze, and atomicrmw all crash at NVPTXLowerArgs.cpp:256 on both sm_60 and sm_90. The crash also occurs with -stop-after=nvptx-lower-args, isolating it to the Lower-Args pass.

Adversarial checks: (a) IR passes the verifier cleanly (opt -passes=verify). (b) Not UB:

## Round-2 corroboration

Re-derived independently. The full set of unhandled pointer consumers that hit
`llvm_unreachable("Unsupported instruction")` at NVPTXLowerArgs.cpp:256 (when no
phi/select is present, taking case 1) is: `icmp`, `freeze`, `atomicrmw`,
`cmpxchg`. Root cause is `ArgUseChecker` lacking a `visitInstruction` override,
so these fall to `InstVisitor`'s no-op default and are misclassified read-only.
See also #005 (the same root cause produces a silent miscompile, not a crash,
when the pointer flows through a phi/select on sm_70+).
