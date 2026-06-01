# 061 — atomicrmw/cmpxchg on shared::cluster (AS 7) emits atom.*.shared::cluster.* with no sm_90/PTX7.8 cluster guard (atom path, distinct from #051's ld/st path)

- **Kind:** other (invalid PTX)
- **Reachable via:** default llc
- **Component:** NVPTXIntrinsics.td 2514-2538 (F_ATOMIC_2), 2540-2578 (F_ATOMIC_3), 2509-2511 (GetAddSp / getAddrSpace), 2583-2638 (base atom defms with preds=[]), 2754-2780 (ATOM_CAS_B128 / ATOM_EXCH_B128)  (round-8 area `X11-red-atom-as2`)
- **Note:** "invalid PTX" entries are validated against the PTX ISA + in-tree corroboration; no local `ptxas` was available to execute the rejection.

## Summary

`atomicrmw`/`cmpxchg` on shared::cluster (AS 7) emits `atom.shared::cluster.*` with no sm_90/PTX7.8 guard (distinct atom path from #051's ld/st)

## Mechanism / root cause

The base integer/float atom instructions are emitted via the F_ATOMIC_2 / F_ATOMIC_3 multiclasses (NVPTXIntrinsics.td:2514, 2540). Their asm string is `"atom${sem:sem}${scope:scope}${addsp:addsp}" # "." # op_str`, and the address-space sub-qualifier comes straight from the `GetAddSp` SDNodeXForm (line 2509-2511) which calls `getAddrSpace(cast<MemSDNode>(N))`. getAddrSpace (NVPTXISelDAGToDAG.cpp:499-514) passes NVPTX::AddressSpace::SharedCluster (numeric AS 7) through unchanged, and the printer's `addsp` modifier (NVPTXInstPrinter.cpp:350-363) prints it verbatim via addressSpaceToString -> "shared::cluster" (NVPTXUtilities.h:167-168). Crucially the multiclasses apply ONLY `Requires<preds>`, and every base defm (INT_PTX_ATOM_ADD_32/64, _SWAP, _AND/_OR/_XOR, _MIN/_MAX/_UMIN/_UMAX, _INC/_DEC, INT_PTX_ATOM_CAS_16/32/64 at lines 2583-2638; and the b128 ATOM_CAS_B128/ATOM_EXCH_B128 guarded only by hasAtomSwap128) passes an EMPTY predicate list for the address-space dimension. There is no `hasClusters` predicate on the atom path. The `.shared::cluster` state space (cluster-scoped distributed shared memory window) only exists from PTX ISA 7.8 / target sm_90+ (Hopper clusters); ptxas rejects `.shared::cluster` under any lower .target/.version. In-tree this exact floor is encoded as `hasClusters() = SmVersion >= 90 && PTXVersion >= 78` (NVPTXSubtarget.h:107), which the SIBLING cvta.shared::cluster (#052) and (should-be) ld/st (#051) paths reference. This is a DIFFERENT code path than #051: #051 is the LD<>/ST<> classes in NVPTXInstrInfo.td; this is the atom F_ATOMIC_* multiclasses in NVPTXIntrinsics.td. It is also disjoint from #040 (const AS4/param AS101) and #054 (cmpxchg local AS5) — those are entirely different address spaces. Confirmed emitted at .target sm_50/.version 4.0, sm_60, sm_70, and the default sm_75/PTX6.3; at sm_90/PTX7.8 the same atom correctly emits .shared::cluster, proving the only defect is the missing guard. (The b128 sub-case `atom.relaxed.sys.shared::cluster.cas.b128` is the same missing-guard root cause with an additional b128-state-space ISA dimension.)

## Trigger

Compile, for any target below sm_90/PTX7.8 (e.g. -mcpu=sm_50, sm_60, sm_70, or the DEFAULT sm_75), a function that does an `atomicrmw` (add/xchg/and/or/xor/min/max/umin/umax/fadd) or `cmpxchg` through a `ptr addrspace(7)` (ADDRESS_SPACE_SHARED_CLUSTER).

## Reproducer

```
target triple = "nvptx64-nvidia-cuda"

define void @add(ptr addrspace(7) %p, i32 %v) {
  %r = atomicrmw add ptr addrspace(7) %p, i32 %v monotonic
  ret void
}
define void @cas(ptr addrspace(7) %p, i32 %c, i32 %v) {
  %r = cmpxchg ptr addrspace(7) %p, i32 %c, i32 %v monotonic monotonic
  ret void
}
define void @exch(ptr addrspace(7) %p, i32 %v) {
  %r = atomicrmw xchg ptr addrspace(7) %p, i32 %v monotonic
  ret void
}
```

Command: `llc -mtriple=nvptx64 -mcpu=sm_50 -o - repro.ll`

## Verification

Reproduced with the built llc (crash/emitted-PTX matches; finder confidence 0.88). 
