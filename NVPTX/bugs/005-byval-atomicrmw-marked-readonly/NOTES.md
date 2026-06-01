# 005 — byval kernel param written via atomicrmw/cmpxchg is misclassified read-only: no local copy, wrongly marked readonly+grid_constant, atomic RMW emitted into shared .param space

- **Kind:** miscompile
- **Reachable via:** default llc, sm_70+
- **Component:** NVPTXLowerArgs.cpp 349-382 (checker, missing atomic handling), 292-307 (createNVVMInternalAddrspaceWrap adds readonly+grid_constant), 447-467 (case 2)  (region `P1-lower-args`)
- **Candidate id:** c007

## Summary

byval kernel param written via atomicrmw/cmpxchg is wrongly marked `readonly`+`grid_constant`, no local copy

## Mechanism / root cause

atomicrmw/cmpxchg are writes through the pointer, but ArgUseChecker has no visitAtomicRMWInst/visitAtomicCmpXchgInst and no visitInstruction override, so these uses hit the no-op default and are classified as neither escaped nor aborted. visitArgPtr thus reports the arg as read-only. If the arg also has a phi/select use (AUC.Conditionals non-empty), case (1) at line 437 is skipped, and on sm_70+ (HasCvtaParam) case (2) at line 448 is entered because `ArgUseIsReadOnly` is (wrongly) true. Case (2) calls createNVVMInternalAddrspaceWrap, which (lines 303-304) unconditionally adds `nvvm.grid_constant` and `readonly` attributes to the argument, then casts the param to generic via cvta.param and RAUWs all uses, creating NO per-thread local copy. Result: (a) the write (atomicrmw) now targets the shared kernel-parameter memory aliased through cvta.param instead of a private per-thread copy, violating byval semantics (each invocation must see its own copy); (b) the argument is given a `readonly` attribute despite being provably written, which downstream IR consumers may exploit to miscompile further. Verified end to end: the select+atomicrmw example below produces `define ... ptr readonly byval(%struct.S) ... "nvvm.grid_constant" %p` after nvptx-lower-args, and full codegen emits `atom.acquire.sys.param.add.u32 %r1, [kern_param_0], 7;` — an atomic add directly into param space with no local copy.

## Trigger

ptx_kernel function, sm_70+ with PTX>=7.7 (hasCvtaParam, e.g. -mcpu=sm_90), a byval pointer arg that is (1) written via atomicrmw or cmpxchg and (2) also flows through a phi or select so the easy read-only path (case 1, which would instead crash) is bypassed. The arg need NOT be __grid_constant__ in the source.

## Reproducer

See `repro.ll` / `cmd.sh`.

```
target triple = "nvptx64-nvidia-cuda"
%struct.S = type { i32, i32 }
define ptx_kernel void @kern(ptr byval(%struct.S) align 8 %p, i1 %c) {
entry:
  %sel = select i1 %c, ptr %p, ptr %p
  %old = atomicrmw add ptr %sel, i32 7 seq_cst
  ret void
}
```

Command:

```
llc -mtriple=nvptx64 -mcpu=sm_90 -O2 -o - repro.ll
```

## Observed (wrong) output

```
.visible .entry kern(
	.param .align 8 .b8 kern_param_0[8],
	.param .u8 kern_param_1
)
{
	.reg .b32 	%r<2>;
// %bb.0:                               // %entry
	fence.sc.sys;
	atom.acquire.sys.param.add.u32 	%r1, [kern_param_0], 7;
	ret;
}

(After nvptx-lower-args the arg becomes: ptr readonly byval(%struct.S) align 8 "nvvm.grid_constant" %p, with NO local alloca/memcpy; the atomicrmw is performed via cvta.param directly on the grid-shared parameter bank.)

For the multithread observable case (c007-multithread.ll, store old value to out[tid]):
	atom.acquire.sys.param.add.u32 	%r1, [kern_mt_param_0], 7;
	mov.u32 	%r2, %tid.x;
	...
	st.global.b32 	[%rd3], %r1;   ; all threads race on shared kern_mt_param_0
```

## Expected

A byval kernel parameter that is written (here via atomicrmw/cmpxchg) must receive a private per-thread local copy (case 3 / copyByValParam): an alloca initialized by memcpy from the param bank, with all uses (including the atomicrmw) redirected to the alloca, and the arg must NOT be marked readonly or grid_constant. Correct lowering would look like the plain-store case:

  %p1 = alloca %struct.S, align 8
  %p.param = call ... @llvm.nvvm.internal.addrspace.wrap.p101.p0(ptr %p)
  call void @llvm.memcpy.p0.p101.i64(ptr align 8 %p1, ptr addrspace(101) align 8 %p.param, i64 8, i1 false)
  %sel = select i1 %c, ptr %p1, ptr %p1
  %old = atomicrmw add ptr %sel, i32 7 seq_cst

i.e. PTX where the atomic targets a per-thread .local copy, not atom...param... on the shared kern_param_0. For the multithread example with N threads and initial value K, every thread must observe old==K and the param bank must be unmodified (out = {K,K,...,K}), instead of the emitted code's racy {K, K+7, K+14, ...} accumulation into shared param space.

Fix: add visitAtomicRMWInst/visitAtomicCmpXchgInst (and/or a conservative visitInstruction) to ArgUseChecker that call PI.setAborted, so a written byval arg is not classified read-only.

## Verification

Verified empirically with the built llc (independent verify + adversarial
refute both `confirmed`, finder confidence 0.85, verify confidence 0.9).

> CONFIRMED miscompile. The cited mechanism is real and reproduces end-to-end.

Source confirmation: ArgUseChecker (PtrUseVisitor subclass) in NVPTXLowerArgs.cpp has no visitAtomicRMWInst/visitAtomicCmpXchgInst and no visitInstruction override. In InstVisitor.h, visitAtomicRMWInst and visitAtomicCmpXchgInst both DELEGATE(Instruction) -> visitInstruction, which is a no-op (line 287). So an atomicrmw/cmpxchg use of a byval arg pointer is classified as NEITHER escaped NOR aborted, making ArgUseIsReadOnly=true at line 436. A select/phi use makes AUC.Conditionals non-empty so case (1) (line 437) is skipped; on sm_70+ (HasCvtaParam), case (2) (line 448) is entered because ArgUseIsReadOnly is (wrongly) true. createNVVMInternalAddrspaceWrap (line 292) unconditionally adds nvvm.grid_constant + readonly (lines 303-304) and RAUWs all uses with cvta.param(arg) -> generic, creating NO per-thread local copy.

This contradicts the pass's own documented contract: header comment lines 88-90 say byval params that *might mutate* must get a local alloca copy; only grid_constant params (where mutation is UB) skip the copy. NVVMProperties.cpp:304-309 states grid_constant lowering 'violates the byval semantics ... by reusing the same memory location for the argument across multiple threads' and is only legal when the arg onlyReadsMemory(). An atomicrmw is a write, so this legality condition is violated, but the misclassification bypasses the isParamGridConstant gate and stamps readonly+grid_constant 

## Round-2 corroboration

The independent class sweep re-derived this from the "incomplete visitor"
class and confirmed `cmpxchg` and `freeze` are affected identically (same
`ArgUseChecker` no-op default → `ArgUseIsReadOnly` true → case-2 cvta.param
lowering with no per-thread local copy). The `freeze` case is slightly worse:
the no-op default also fails to enqueue the frozen pointer's users, so a
`store` through `freeze ptr %p` is entirely invisible to the checker.
