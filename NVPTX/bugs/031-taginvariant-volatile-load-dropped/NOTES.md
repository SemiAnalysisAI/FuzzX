# 031 — NVPTXTagInvariantLoads tags volatile loads as !invariant.load, dropping volatile semantics (lowered to ld.global.nc)

- **Kind:** miscompile
- **Reachable via:** default llc, sm_70+
- **Component:** NVPTXTagInvariantLoads.cpp 33-58, 60-81  (round-5 area `V10-lowerargs-nonbyval`)
- **Candidate id:** r5_04

## Summary

NVPTXTagInvariantLoads tags a `load volatile` (from a noalias readonly arg) `!invariant.load`, lowering it to `ld.global.nc` and dropping volatile

## Mechanism / root cause

isInvariantLoad() never checks LI->isVolatile(). A `load volatile` from a `noalias readonly` kernel pointer arg in global AS therefore passes the predicate (lines 50-57) and gets stamped !invariant.load (markLoadsAsInvariant, lines 60-63). In ISel, canLowerToLDG (NVPTXISelDAGToDAG.cpp:785-786) returns true purely on N.isInvariant() (no isVolatile check), so the volatile load is lowered via tryLDG to `ld.global.nc` (the non-coherent read-only data cache). This silently drops volatile semantics: the access becomes cacheable, freely reorderable/CSE-able, and reads through a cache that PTX explicitly says is only valid for data not written during the kernel. Correct lowering (no invariant tag) is `ld.volatile.global.b32`. A volatile poll of memory written by another agent (e.g. a spin flag) would read a stale cached value. Confirmed by diffing: with noalias readonly -> `ld.global.nc.b32`; with a plain (non-readonly) arg the same volatile load -> `ld.volatile.global.b32`.

## Trigger

ptx_kernel with `noalias readonly` pointer arg; `load volatile i32` from it in addrspace(1); compile for sm_80. Output uses ld.global.nc (volatile dropped) instead of ld.volatile.global.

## Reproducer

```
target triple = "nvptx64-unknown-cuda"

define ptx_kernel void @volload(ptr noalias readonly %a, ptr %out) {
  %ag = addrspacecast ptr %a to ptr addrspace(1)
  %v = load volatile i32, ptr addrspace(1) %ag, align 4
  store i32 %v, ptr %out, align 4
  ret void
}
```

Command: `llc -mtriple=nvptx64 -mcpu=sm_80 -o - repro.ll`

## Observed (wrong) output

```
ld.param.b64 	%rd1, [volload_param_0];
	cvta.to.global.u64 	%rd2, %rd1;
	ld.param.b64 	%rd3, [volload_param_1];
	cvta.to.global.u64 	%rd4, %rd3;
	ld.global.nc.b32 	%r1, [%rd2];      // <-- volatile load lowered to non-coherent cached read (volatile dropped)
	st.global.b32 	[%rd4], %r1;
	ret;

(Spin-wait variant r5_04-spin.ll emits `ld.global.nc.b32` inside the polling loop body. Removing `readonly` from the arg changes the instruction to the correct `ld.volatile.global.b32` in both cases — readonly is the sole trigger. opt -passes=nvptx-tag-invariant-loads on the input produces: `%v = load volatile i32, ptr addrspace(1) %ag, align 4, !invariant.load !0`.)
```

## Expected

The volatile load must NOT be tagged !invariant.load and must lower to `ld.volatile.global.b32 %r1, [%rd2];` (an uncached, side-effecting, non-reorderable read that re-reads memory on every execution), exactly as it does when the `readonly` attribute is absent. ld.global.nc reads through the non-coherent read-only data cache and is only valid for data not written during the kernel, which violates volatile's requirement to observe writes from other agents and to not be CSE'd/hoisted.

## Verification

Independent verify + adversarial refute, both `confirmed` (verify confidence 0.95).

> Confirmed real miscompile. Mechanism verified end-to-end:

1. NVPTXTagInvariantLoads.cpp:isInvariantLoad (lines 33-58) never checks LI->isVolatile(). A `load volatile i32` from a `noalias readonly` kernel-arg pointer in addrspace(1) satisfies the predicate at line 52 (A->onlyReadsMemory() && A->hasNoAliasAttr()) and is stamped !invariant.load by markLoadsAsInvariant (lines 60-63). Verified directly: `opt -passes=nvptx-tag-invariant-loads` emits `load volatile i32, ... !invariant.load !0` (a contradictory volatile+invariant load).

2. TargetLoweringBase::getLoadMemOperandFlags (TargetLoweringBase.cpp:2790-2797) sets BOTH MOVolatile and MOInvariant on the same MMO. So the MemSDNode has isVolatile()==true AND isInvariant()==true.

3. NVPTXISelDAGToDAG.cpp:canLowerToLDG (lines 781-787) returns true purely on Subtarget.hasLDG() && global AS && N.isInvariant() — NO volatile check. tryLoad (line 1122) calls this BEFORE the volatile fence/ordering logic (line 1127), so it routes to tryLDG and emits `ld.global.nc` (non-coherent read-only data-cache load), silently dropping volatile semantics.

Empirical proof (llc -mcpu=sm_80):
- readonly arg:    `ld.global.nc.b32`     (volatile dropped, ca
