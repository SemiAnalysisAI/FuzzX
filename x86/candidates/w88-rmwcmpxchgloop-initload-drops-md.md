file: llvm/lib/CodeGen/AtomicExpandPass.cpp:1769-1789
(`insertRMWCmpXchgLoop`)

The InitLoaded created with `CreateAlignedLoad` for the
cmpxchg-loop expansion only inherits whatever metadata
`ReplacementIRBuilder` propagates by default — `!pcsections`. It
does NOT get a `copyMetadataForAtomic` call. The cmpxchg INSIDE
the loop does get metadata via `copyMetadataForAtomic(*Pair,
*MetadataSrc)` at AtomicExpandPass.cpp:758 / line 2023.

So the user-attached AA metadata (`!noalias`, `!tbaa`,
`!alias.scope`, `!access_group`, `!mmra`, …) lives on the cmpxchg
but is silently dropped on the InitLoaded for the SAME memory
location, accessed from the SAME source instruction.

Reproducer (X86 fadd → CmpXchg expansion):

  define float @fadd_md(ptr %p, float %v) {
    %x = atomicrmw fadd ptr %p, float %v seq_cst, align 4,
                   !pcsections !0, !noalias !2
    ret float %x
  }
  !0 = !{!1}
  !1 = !{!"pc"}
  !2 = !{!3}
  !3 = distinct !{!3, !4, !"s"}
  !4 = distinct !{!4, !"d"}

`opt -mtriple=x86_64-unknown-linux-gnu -atomic-expand -S` output:

  define float @fadd_md(ptr %p, float %v) {
    %1 = load float, ptr %p, align 4, !pcsections !0           ; no !noalias
    ...
    %4 = cmpxchg ptr %p, i32 %3, i32 %2 seq_cst seq_cst, align 4,
                  !noalias !2, !pcsections !0                   ; !noalias kept here
    ...
  }

The InitLoaded loses `!noalias !2`. Same site, same address —
inconsistent AA view between the two atomic accesses. An AA query
that uses the load may report MayAlias where the source promised
NoAlias, silently disabling later optimizations or — for
`!noalias.addrspace` — risking incorrect memory-effect
classifications.

Fix: after `CreateAlignedLoad(...)` in insertRMWCmpXchgLoop, call
`copyMetadataForAtomic(*InitLoaded, *MetadataSrc)` when
`MetadataSrc` is non-null. Same patch needed in
`expandPartwordCmpXchg` (line 1221) and in
`lowerIdempotentRMWIntoFencedLoad` (X86ISelLowering.cpp:33053-55),
which only copies `MD_pcsections` and drops the rest.
