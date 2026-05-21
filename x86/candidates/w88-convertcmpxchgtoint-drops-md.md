file: llvm/lib/CodeGen/AtomicExpandPass.cpp:1417-1449

`convertCmpXchgToIntegerType` rewrites a pointer-typed cmpxchg
into the equivalent integer cmpxchg. It propagates:
  - alignment, success/failure ordering, syncscope (in CreateAtomicCmpXchg)
  - volatile flag (`setVolatile`)
  - weak flag (`setWeak`)

It does NOT call `copyMetadataForAtomic` and does NOT explicitly
propagate any metadata other than what `ReplacementIRBuilder`
auto-attaches (only `!pcsections`).

Metadata dropped on the new cmpxchg includes:
  !tbaa, !tbaa.struct, !alias.scope, !noalias, !noalias.addrspace,
  !access_group, !mmra
(everything else `copyMetadataForAtomic` would have preserved per
AtomicExpandPass.cpp:233-260).

Reproducer:

  define { ptr, i1 } @cas_ptr_with_md(ptr %p, ptr %c, ptr %n) {
    %x = cmpxchg ptr %p, ptr %c, ptr %n seq_cst seq_cst, align 8,
                  !pcsections !0, !noalias !2
    ret { ptr, i1 } %x
  }
  !0 = !{!1}
  !1 = !{!"pcsec"}
  !2 = !{!3}
  !3 = distinct !{!3, !4, !"scope"}
  !4 = distinct !{!4, !"dom"}

`opt -mtriple=x86_64-unknown-linux-gnu -atomic-expand -S` yields:

  %3 = cmpxchg ptr %p, i64 %1, i64 %2 seq_cst seq_cst, align 8, !pcsections !0

`!noalias !2` is gone. (Same fate for `!tbaa`, `!alias.scope`, etc.)

Consequence: a later pass may decide the cmpxchg aliases with
something that the original `!noalias` had excluded, blocking a
legal hoist/sink/dead-store-elimination. The flip side — losing
*restrictive* metadata — is generally only a missed optimization,
but losing `!tbaa` can also affect AA correctness in passes that
rely on it for disambiguation.

Fix: in convertCmpXchgToIntegerType, add
  `copyMetadataForAtomic(*NewCI, *CI);`
mirroring what `convertAtomicXchgToIntegerType` already does at
line 598.
