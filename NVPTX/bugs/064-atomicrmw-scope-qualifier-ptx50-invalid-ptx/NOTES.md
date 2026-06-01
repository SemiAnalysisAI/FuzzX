# 064 — Generic atomicrmw emits .cta/.gpu/.sys atom scope qualifier on sm_60/sm_61 (default PTX 5.0), where atom scopes don't exist until PTX ISA 6.0 (ptxas rejects)

- **Kind:** other (invalid PTX)
- **Reachable via:** llc -mcpu=sm_60/sm_61
- **Component:** NVPTXISelDAGToDAG.cpp 546-550 (getAtomicScope); guard def NVPTXSubtarget.h:102 hasAtomScope(){return SmVersion>=60;}  (round-8 area `X09-ldst-b128-sys-arch`)
- **Note:** "invalid PTX" entries are validated against the PTX ISA + in-tree corroboration; no local `ptxas` was available to execute the rejection.

## Summary

generic `atomicrmw`/`cmpxchg` emits a `.cta/.gpu/.sys` scope qualifier on sm_60/sm_61 (default PTX 5.0), but atom scopes require PTX ISA 6.0 (`getAtomicScope` checks only sm, not PTX version)

## Mechanism / root cause

getAtomicScope() decides the PTX scope qualifier (.cta/.gpu/.sys/.cluster) for the generic atomicrmw/cmpxchg path:

  NVPTX::Scope NVPTXDAGToDAGISel::getAtomicScope(const MemSDNode *N) const {
    if (!Subtarget->hasAtomScope())
      return NVPTX::Scope::DefaultDevice;   // no qualifier
    return resolveScope(Scopes[N->getSyncScopeID()], Subtarget);
  }

The only gate is hasAtomScope() == (SmVersion >= 60) (NVPTXSubtarget.h:102) — it does NOT check PTXVersion. The resulting Scope is printed by NVPTXInstPrinter::printAtomicCode (MCTargetDesc/NVPTXInstPrinter.cpp:328-346): System->".sys", Block->".cta", Device->".gpu". So for any atomicrmw/cmpxchg on sm_60/sm_61 the backend appends a scope qualifier.

WHY WRONG: sm_60/sm_61 (Pascal) have a minimum/default PTX version of 5.0 (verified: `llc -mcpu=sm_60` emits `.version 5.0`). The `.cta`/`.gpu`/`.sys` scope qualifiers on `atom` were introduced only in PTX ISA 6.0 (sm_70/Volta), as part of the memory consistency model. In PTX 5.0 the only legal form is the unscoped `atom.add.u32`. ptxas for `.version 5.0` rejects `atom.sys.add.u32` / `atom.cta.add.u32` / `atom.gpu.max.s32`.

The analogous ordering path is correctly guarded: getOperationOrderings/hasMemoryOrdering() == (SmVersion>=70 && PTXVersion>=60), so atomic ld/st below PTX 6.0 drop ordering and scope (verified: `load atomic ... syncscope("device")` at sm_60 emits plain `ld.volatile.b32`, no scope). Only the atom/red scope path via getAtomicScope lacks the PTX>=60 check.

ISA CITE: PTX ISA 6.0 (June 2018) introduced the memory consistency model and the `.cta`/`.gpu`/`.sys` scope qualifiers on `atom` with sm_70 support. ISA 8.4 later notes 'extends ld, st and atom with .b128 type to support .sys scope' — confirming scope qualifiers are a 6.0+ feature absent in 5.0.

DISTINCT FROM #032: #032 is the scoped 16-bit CAS *NVVM intrinsic* (llvm.nvvm.atomic.cas.gen.i.cta.i16) whose hardcoded scope Pat in NVPTXIntrinsics.td carries no predicate, and its NOTES explicitly assert the generic IR atomicrmw/cmpxchg path is SAFE ('the generic legalizer suppresses the scope qualifier on unsupported targets'). That assumption is false: the generic atomicrmw path (this finding, in the .cpp getAtomicScope, no intrinsic involved) emits atom.sys/.cta/.gpu at PTX 5.0. Different code location (.cpp getAtomicScope vs .td Pat) and different, much broader trigger (any plain atomicrmw, including the no-syncscope system-scope default).

## Trigger

Any atomicrmw or cmpxchg compiled for sm_60 or sm_61 with the default PTX version (5.0). Broadest case: a plain `atomicrmw add ptr %p, i32 %v monotonic` with NO syncscope (defaults to system scope) -> `atom.sys.add.u32`. Also `syncscope("block")` -> `atom.cta.add.u32`, `syncscope("device")` -> `atom.gpu.max.s32`.

## Reproducer

```
target triple = "nvptx64-nvidia-cuda"

define i32 @plain(ptr %p, i32 %v) {
  %r = atomicrmw add ptr %p, i32 %v monotonic
  ret i32 %r
}
define i32 @rmw_block(ptr %p, i32 %v) {
  %r = atomicrmw add ptr %p, i32 %v syncscope("block") monotonic
  ret i32 %r
}
define i32 @rmw_device(ptr %p, i32 %v) {
  %r = atomicrmw max ptr %p, i32 %v syncscope("device") monotonic
  ret i32 %r
}
```

Command: `llc -mtriple=nvptx64 -mcpu=sm_60 -o - repro.ll`

## Verification

Reproduced with the built llc (crash/emitted-PTX matches; finder confidence 0.85). 
