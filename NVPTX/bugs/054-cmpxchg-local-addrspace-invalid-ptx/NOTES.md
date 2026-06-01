# 054 — cmpxchg on a local (addrspace 5) pointer emits unassemblable atom.local.cas.b{32,64,128} (atom does not support the .local state space)

- **Kind:** other (invalid PTX)
- **Reachable via:** default llc
- **Component:** NVPTXIntrinsics.td NVPTXISelDAGToDAG.cpp:499-513 (getAddrSpace returns Local unfiltered); NVPTXInstPrinter.cpp:350-363 (printAtomicCode "addsp" prints ".local"); NVPTXIntrinsics.td:2540-2578 (F_ATOMIC_3 cas asm "atom...${addsp:addsp}.cas"), 2633-2639 (INT_PTX_ATOM_CAS_*), 2756-2767 (ATOM_CAS_B128)  (round-7 area `A01-atom-red-as`)
- **Note:** "invalid PTX" entries are validated against the PTX ISA + in-tree corroboration (sibling guards/orderings); no local `ptxas` was available to execute the rejection.

## Summary

`cmpxchg` on a local (AS 5) pointer emits `atom.local.cas.b{32,64,128}` — atom does not support the `.local` state space (distinct from #040, which dismissed local)

## Mechanism / root cause

The cmpxchg lowering selects the F_ATOMIC_3 / ATOM_CAS_B128 patterns whose asm string is "atom${sem:sem}${scope:scope}${addsp:addsp}.cas.bN". The addsp operand comes from getAddrSpace(N) (NVPTXISelDAGToDAG.cpp:499-513), which returns the raw memory-operand address space with NO filtering — Local (NVPTX::AddressSpace::Local == addrspace 5) is in the accepted list and returned as-is. printAtomicCode's "addsp" case (NVPTXInstPrinter.cpp:350-363) then unconditionally prints "." + addressSpaceToString(Local) = ".local". Result: e.g. `atom.relaxed.sys.local.cas.b32`. Per PTX ISA 8.8/9.x sec 9.7.13.5, atom's `.space` is restricted to `{ .global, .shared{::cta,::cluster} }` and "atom ... may be used only with .global and .shared spaces and with generic addressing"; `.local` is thread-private and is NOT a legal atom state space, so ptxas rejects `atom.local.*`. NOTE this is a DISTINCT instance from found-bug #040: #040 covers only const(AS4)/param(AS101) (and notes shared_cluster), and #040's NOTES explicitly DISMISS local as safe ("Local, addrspace 5, is usually optimized to plain ld/st since it is thread-private"). That dismissal is only true for atomicrmw (which is folded to ld.local/st.local); the cmpxchg instruction is NOT expanded for local — there is no local special-case in shouldInsertFencesForAtomic / the cmpxchg lowering — so it reaches the atom.cas pattern and emits the invalid `.local` qualifier. Affects b32 (direct cmpxchg i32), b64 (cmpxchg i64), b16 (expanded to a b32 CAS loop still on .local), and b128 (ATOM_CAS_B128, line 2756). The atom/red `red.*` reduction question in the task brief is moot: the NVPTX backend emits no PTX `red` instruction at all (reductions always lower to result-discarding `atom`), so the addsp printer is reached only via atom.

## Trigger

A type-correct, non-UB `cmpxchg` whose pointer is in the local address space (addrspace(5)). Minimal: `cmpxchg ptr addrspace(5) %p, i32 %c, i32 %n monotonic monotonic`. Reproduces on the default target (sm_75/ptx6.3) and every sm/ptx tested; independent of any runtime UB (a cmpxchg on a thread-local location is well-defined IR).

## Reproducer

```
target triple = "nvptx64-nvidia-cuda"
define {i32, i1} @local_cas(ptr addrspace(5) %p, i32 %c, i32 %n) {
  %r = cmpxchg ptr addrspace(5) %p, i32 %c, i32 %n monotonic monotonic
  ret {i32, i1} %r
}
define i64 @cas64(ptr addrspace(5) %p, i64 %c, i64 %n) {
  %r = cmpxchg ptr addrspace(5) %p, i64 %c, i64 %n monotonic monotonic
  %v = extractvalue {i64,i1} %r, 0
  ret i64 %v
}
define i128 @cas128(ptr addrspace(5) %p, i128 %c, i128 %n) {
  %r = cmpxchg ptr addrspace(5) %p, i128 %c, i128 %n monotonic monotonic
  %v = extractvalue {i128,i1} %r, 0
  ret i128 %v
}
```

Command: `llc -mtriple=nvptx64 -mcpu=sm_90 -mattr=+ptx83 -o - repro.ll`

## Verification

Reproduced with the built llc (crash/emitted-PTX matches the claim; finder confidence 0.78).
