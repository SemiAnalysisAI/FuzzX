# 052 — addrspacecast to/from shared::cluster emits cvta.shared::cluster below sm_90/PTX7.8 (C++ selection bypasses [hasClusters] predicate)

- **Kind:** other (invalid PTX)
- **Reachable via:** default llc
- **Component:** NVPTXISelDAGToDAG.cpp 956-960 (NG_TO_G), 987-991 (G_TO_NG) in SelectAddrSpaceCast  (round-7 area `A02-ldst-cvta-special-as`)
- **Note:** "invalid PTX" entries are validated against the PTX ISA + in-tree corroboration (sibling guards/orderings); no local `ptxas` was available to execute the rejection.

## Summary

`addrspacecast` to/from shared::cluster emits `cvta.shared::cluster` below sm_90 — the C++ `getMachineNode` path bypasses the `[hasClusters]` predicate

## Mechanism / root cause

SelectAddrSpaceCast() lowers an `addrspacecast` involving SHARED_CLUSTER by directly calling `CurDAG->getMachineNode(NVPTX::cvta_shared_cluster_64, ...)` / `cvta_to_shared_cluster_64`. The TableGen defs `cvta_shared_cluster`/`cvta_to_shared_cluster` (NVPTXIntrinsics.td:2902-2903) carry `Requires<[hasClusters]>` (= SmVersion>=90 && PTXVersion>=78), but that predicate is only consulted during *pattern-matched* selection. These defs have no DAG pattern and are emitted via the manual getMachineNode path, which ignores instruction predicates entirely. Result: on sm_50/.version 4.0 the backend emits `cvta.shared::cluster.u64` / `cvta.to.shared::cluster.u64`, which per PTX ISA require PTX 7.8 / sm_90 — invalid PTX, rejected by ptxas. This is a separate code path from the ld/st shared::cluster issue and from bug #037 (cvta.param).

## Trigger

A well-defined `addrspacecast ptr addrspace(7) -> ptr` or `addrspacecast ptr -> ptr addrspace(7)` compiled for any 64-bit target below sm_90 / PTX 7.8 (e.g. -mcpu=sm_50). (32-bit nvptx is separately report_fatal_error'd.)

## Reproducer

```
target triple = "nvptx64-nvidia-cuda"
define ptr @cast_cluster_to_generic(ptr addrspace(7) %p) {
  %c = addrspacecast ptr addrspace(7) %p to ptr
  ret ptr %c
}
define ptr addrspace(7) @cast_generic_to_cluster(ptr %p) {
  %c = addrspacecast ptr %p to ptr addrspace(7)
  ret ptr addrspace(7) %c
}
```

Command: `llc -mtriple=nvptx64 -mcpu=sm_50 -o - repro.ll`

## Verification

Reproduced with the built llc (crash/emitted-PTX matches the claim; finder confidence 0.85).
