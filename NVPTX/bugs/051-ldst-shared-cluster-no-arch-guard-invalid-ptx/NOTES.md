# 051 — ld/st to shared::cluster address space (AS 7) emits .shared::cluster qualifier with no sm_90/PTX7.8 arch guard

- **Kind:** other (invalid PTX)
- **Reachable via:** default llc
- **Component:** NVPTXInstrInfo.td 1939-1968 (LD/ST classes, no Requires); selection in NVPTXISelDAGToDAG.cpp tryLoad 1111-1180 / tryStore 1383-1435  (round-7 area `A02-ldst-cvta-special-as`)
- **Note:** "invalid PTX" entries are validated against the PTX ISA + in-tree corroboration (sibling guards/orderings); no local `ptxas` was available to execute the rejection.

## Summary

`ld`/`st` to shared::cluster (AS 7) emit `.shared::cluster` with no sm_90/PTX7.8 guard on the LD/ST classes

## Mechanism / root cause

The LD<>/ST<> instruction classes (NVPTXInstrInfo.td:1939-1968) carry NO `Requires<>` subtarget predicate; the address-space sub-qualifier is whatever getAddrSpace() returns, printed verbatim by addressSpaceToString (NVPTXUtilities.h:167-168 -> "shared::cluster"). A load/store to `addrspace(7)` therefore emits `ld.shared::cluster.bN` / `st.shared::cluster.bN` regardless of the selected target. Per PTX ISA 7.8 release notes the `.shared::cluster` sub-qualifier (cluster-scoped shared window) is introduced only at PTX ISA 7.8 and target sm_90+. Emitting it under `.version 4.0`/`.target sm_50` is invalid PTX that ptxas rejects. Note the sibling `cvta.shared::cluster` form IS guarded ([hasClusters] = sm_90 && PTX78), so the omission of any guard on the plain ld/st path is an inconsistency. Even at the default target (sm_75, .version 6.3) it is still emitted below the 7.8/sm_90 floor.

## Trigger

A well-defined `load`/`store` through a `ptr addrspace(7)` (SHARED_CLUSTER) compiled for any target below sm_90 / PTX 7.8 (e.g. -mcpu=sm_50, sm_60, or the default sm_75).

## Reproducer

```
target triple = "nvptx64-nvidia-cuda"
define void @st_cluster(ptr addrspace(7) %p, i32 %v) {
  store i32 %v, ptr addrspace(7) %p
  ret void
}
define i32 @ld_cluster(ptr addrspace(7) %p) {
  %v = load i32, ptr addrspace(7) %p
  ret i32 %v
}
```

Command: `llc -mtriple=nvptx64 -mcpu=sm_50 -o - repro.ll`

## Verification

Reproduced with the built llc (crash/emitted-PTX matches the claim; finder confidence 0.85).
