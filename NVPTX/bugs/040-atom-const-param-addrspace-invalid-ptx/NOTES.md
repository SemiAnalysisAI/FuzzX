# 040 — atomicrmw/cmpxchg on .const/.param/.local address spaces emits unassemblable atom.const/atom.param/atom.local PTX (atom only supports .global/.shared/generic)

- **Kind:** other (invalid PTX)
- **Reachable via:** default llc
- **Component:** NVPTXIntrinsics.td:2514-2538 (F_ATOMIC_2, asm "atom...${addsp:addsp}.<op>") NVPTXInstPrinter.cpp:350-363; NVPTXISelDAGToDAG.cpp:499-513  (round-6 area `W09-atom-red-full`)
- **Note:** "invalid PTX" entries are validated against the PTX ISA + strong in-tree corroboration (sibling guards / orderings); no local `ptxas` was available to execute the rejection.

## Summary

`atomicrmw`/`cmpxchg` on a const(AS4)/param(AS101) pointer emits `atom.const.*`/`atom.param.*` — atom only supports .global/.shared/generic

## Mechanism / root cause

The generic atomicrmw/cmpxchg lowering (F_ATOMIC_2/F_ATOMIC_3, asm string "atom${sem:sem}${scope:scope}${addsp:addsp}.<op>") takes the addsp operand from getAddrSpace(N), which returns the raw memory-operand address space unfiltered (NVPTXISelDAGToDAG.cpp:499-513 lists Const/Local/EntryParam/DeviceParam/SharedCluster among the returned values, no diagnostic). printAtomicCode's "addsp" case (NVPTXInstPrinter.cpp:352-363) then unconditionally prints ".const"/".local"/".param"/".shared::cluster" for those spaces. But the PTX ISA (8.8 sec 9.7.13.5, verified in the downloaded ptx_isa_8.8.pdf: ".space = { .global, .shared{::cta,::cluster} }" and "atom with scalar type may be used only with .global and .shared spaces and with generic addressing") allows ONLY .global, .shared, or generic addressing for atom. There is no state-space restriction anywhere on the atom selection/print path, so a perfectly type-valid atomicrmw to a const (addrspace 4) or param (addrspace 101) pointer compiles to e.g. `atom.const.add.u32` / `atom.param.add.u32`, which ptxas rejects (no such state-space qualifier for atom). The supported spaces (global/shared/generic) are handled correctly; this is purely the unsupported-space class. (Local, addrspace 5, is usually optimized to plain ld/st since it is thread-private, but const and param reach the atom printer directly.)

## Trigger

Any atomicrmw or cmpxchg whose pointer is in the const (addrspace 4) or param (addrspace 101) address space (or shared_cluster addrspace 7 on a non-cluster target). Minimal: `atomicrmw add ptr addrspace(4) %p, i32 %v monotonic`. The IR is well-formed and type-correct; the invalid PTX is produced unconditionally at compile time independent of any runtime UB (and for param space there is no read-only-memory UB story at all).

## Reproducer

```
target triple = "nvptx64-nvidia-cuda"
define i32 @c_add(ptr addrspace(4) %p, i32 %v) {
  %r = atomicrmw add ptr addrspace(4) %p, i32 %v monotonic
  ret i32 %r
}
define i32 @p_add(ptr addrspace(101) %p, i32 %v) {
  %r = atomicrmw add ptr addrspace(101) %p, i32 %v monotonic
  ret i32 %r
}
define i32 @c_xchg(ptr addrspace(4) %p, i32 %v) {
  %r = atomicrmw xchg ptr addrspace(4) %p, i32 %v monotonic
  ret i32 %r
}
define {i32,i1} @c_cas(ptr addrspace(4) %p, i32 %c, i32 %n) {
  %r = cmpxchg ptr addrspace(4) %p, i32 %c, i32 %n monotonic monotonic
  ret {i32,i1} %r
}
```

Command: `llc -mtriple=nvptx64 -mcpu=sm_50 -mattr=+ptx50 -o - repro.ll`

## Verification

Reproduced with the built llc (emitted PTX / crash matches the claim; finder confidence 0.6, confirmed_with_llc=True).
