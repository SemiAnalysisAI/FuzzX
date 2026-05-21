# w108: AtomicExpandPass expandPartwordCmpXchg widened cmpxchg drops !tbaa/!noalias/!pcsections

## Root cause
`AtomicExpandImpl::expandPartwordCmpXchg` (llvm/lib/CodeGen/AtomicExpandPass.cpp:1247-1256)
widens a sub-word `cmpxchg` to a word-sized one:

```
AtomicCmpXchgInst *NewCI = Builder.CreateAtomicCmpXchg(
    PMV.AlignedAddr, FullWord_Cmp, FullWord_NewVal, PMV.AlignedAddrAlignment,
    CI->getSuccessOrdering(), CI->getFailureOrdering(), CI->getSyncScopeID());
NewCI->setVolatile(CI->isVolatile());
NewCI->setWeak(CI->isWeak());
// BUG: no copyMetadataForAtomic(*NewCI, *CI)
```

Contrast with the sibling helper at line 1141-1145, `widenPartwordAtomicRMW`,
which explicitly does:

```
AtomicRMWInst *NewAI = Builder.CreateAtomicRMW(
    Op, PMV.AlignedAddr, NewOperand, PMV.AlignedAddrAlignment,
    AI->getOrdering(), AI->getSyncScopeID());
copyMetadataForAtomic(*NewAI, *AI);   // <-- correctly preserves metadata
```

The cmpxchg-widening path forgot the parallel call. The
`ReplacementIRBuilder` constructor only auto-copies `!pcsections` and
`!annotation`; the entire `copyMetadataForAtomic` family (tbaa, tbaa.struct,
alias.scope, noalias, noalias.addrspace, access_group, mmra,
amdgpu.no.remote.memory) is dropped.

Additionally, the pre-loop seed load `InitLoaded` at line 1221 has the same
defect:

```
LoadInst *InitLoaded = Builder.CreateLoad(PMV.WordType, PMV.AlignedAddr);
```

No metadata copy at all. (Same bug as w108-insertRMWCmpXchgLoop, different
call site.)

## Trigger condition
`expandPartwordCmpXchg` runs when the cmpxchg value type is narrower than
`TLI->getMinCmpXchgSizeInBits() / 8`. X86 does not override
`getMinCmpXchgSizeInBits` so this path is not directly triggered on x86; it
is the dominant path on AArch64/ARM/PPC for sub-word cmpxchg. However the
function is in target-independent code and the bug applies to any of those
backends.

For ARM-style reproducer (uses the same source file):

```
target triple = "armv7-unknown-linux-gnueabi"

define {i8, i1} @test_partword_cmpxchg(ptr %p, i8 %cmp, i8 %new) {
  %r = cmpxchg ptr %p, i8 %cmp, i8 %new seq_cst seq_cst, align 1, !tbaa !2, !noalias !6
  ret {i8, i1} %r
}

!0 = !{!"alias-domain"}
!1 = !{!"alias-scope-a", !0}
!2 = !{!3, !3, i64 0}
!3 = !{!"int", !4, i64 0}
!4 = !{!"omnipotent char", !5, i64 0}
!5 = !{!"Simple C/C++ TBAA"}
!6 = !{!1}
```

After `llc -mtriple=armv7-unknown-linux-gnueabi -stop-after=atomic-expand`:

```
define { i8, i1 } @test_partword_cmpxchg(ptr %p, i8 %cmp, i8 %new) {
  ...
  %InitLoaded = load i32, ptr %AlignedAddr                       ; no !tbaa, no !noalias
  ...
  %NewCI = cmpxchg ptr %AlignedAddr, i32 %FullWord_Cmp, i32 %FullWord_NewVal seq_cst seq_cst, align 4
                                                                  ; ^^^ no !tbaa, no !noalias
  ...
}
```

Both the seed load and the widened cmpxchg lose TBAA / noalias / pcsections,
while the analogous `atomicrmw or` path through `widenPartwordAtomicRMW`
correctly preserves them.

## Why this is a miscompile
- The widened cmpxchg now reads/writes a word but the analysis pipeline no
  longer knows the TBAA tag of the originally-narrowed access. A subsequent
  TBAA-aware pass (e.g. LICM, MemCpyOpt, GVN under LTO recompile) may
  reorder a TBAA-distinct store past the widened cmpxchg.
- The dropped `!noalias` permits a later pass to sink a noalias-protected
  load BETWEEN the InitLoaded seed and the widened cmpxchg -- crossing the
  original cmpxchg boundary that the noalias scope was protecting.
- `!pcsections` dropped on the widened cmpxchg means the post-RTL pcsections
  emitter no longer records the patchable instruction at the right address.

## Fix
1. Line 1250: insert `copyMetadataForAtomic(*NewCI, *CI);` after the
   `setWeak` call. Match `widenPartwordAtomicRMW` exactly.
2. Line 1235 (after `setVolatile` on `InitLoaded`): insert
   `copyMetadataForAtomic(*InitLoaded, *CI);`.

## Related bugs
- Sibling of `widenPartwordAtomicRMW` (line 1145) which gets this right.
- #088 (`w88-rmwcmpxchgloop-initload-drops-md.md`): same family for the
  non-partword `insertRMWCmpXchgLoop`.
- #024 (`w24-widenpartword-atomicrmw-drops-volatile.md`): predecessor
  metadata/volatile bug in same source file.
- This entry is target-independent; reproducer chosen for ARM because
  X86 does not exercise `expandPartwordCmpXchg`.
