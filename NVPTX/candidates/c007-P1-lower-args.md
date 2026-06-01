# c007 — byval kernel param written via atomicrmw/cmpxchg is misclassified read-only: no local copy, wrongly marked readonly+grid_constant, atomic RMW emitted into shared .param space

- region: P1-lower-args
- file: NVPTXLowerArgs.cpp 349-382 (checker, missing atomic handling), 292-307 (createNVVMInternalAddrspaceWrap adds readonly+grid_constant), 447-467 (case 2)
- kind: miscompile
- confidence(finder): 0.85

## Mechanism
atomicrmw/cmpxchg are writes through the pointer, but ArgUseChecker has no visitAtomicRMWInst/visitAtomicCmpXchgInst and no visitInstruction override, so these uses hit the no-op default and are classified as neither escaped nor aborted. visitArgPtr thus reports the arg as read-only. If the arg also has a phi/select use (AUC.Conditionals non-empty), case (1) at line 437 is skipped, and on sm_70+ (HasCvtaParam) case (2) at line 448 is entered because `ArgUseIsReadOnly` is (wrongly) true. Case (2) calls createNVVMInternalAddrspaceWrap, which (lines 303-304) unconditionally adds `nvvm.grid_constant` and `readonly` attributes to the argument, then casts the param to generic via cvta.param and RAUWs all uses, creating NO per-thread local copy. Result: (a) the write (atomicrmw) now targets the shared kernel-parameter memory aliased through cvta.param instead of a private per-thread copy, violating byval semantics (each invocation must see its own copy); (b) the argument is given a `readonly` attribute despite being provably written, which downstream IR consumers may exploit to miscompile further. Verified end to end: the select+atomicrmw example below produces `define ... ptr readonly byval(%struct.S) ... "nvvm.grid_constant" %p` after nvptx-lower-args, and full codegen emits `atom.acquire.sys.param.add.u32 %r1, [kern_param_0], 7;` — an atomic add directly into param space with no local copy.

## Trigger
ptx_kernel function, sm_70+ with PTX>=7.7 (hasCvtaParam, e.g. -mcpu=sm_90), a byval pointer arg that is (1) written via atomicrmw or cmpxchg and (2) also flows through a phi or select so the easy read-only path (case 1, which would instead crash) is bypassed. The arg need NOT be __grid_constant__ in the source.

## IR
```
target triple = "nvptx64-nvidia-cuda"
%struct.S = type { i32, i32 }
define ptx_kernel void @kern(ptr byval(%struct.S) align 8 %p, i1 %c, ptr addrspace(1) %other) {
entry:
  %sel = select i1 %c, ptr %p, ptr %p
  %old = atomicrmw add ptr %sel, i32 7 seq_cst
  ret void
}
```

## llc cmd
`-mtriple=nvptx64 -mcpu=sm_90 -O2`
