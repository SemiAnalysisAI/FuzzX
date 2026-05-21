# w108: AtomicExpandPass insertRMWCmpXchgLoop InitLoaded drops !tbaa/!noalias/!alias.scope

## Root cause
`AtomicExpandImpl::insertRMWCmpXchgLoop` (llvm/lib/CodeGen/AtomicExpandPass.cpp:1769)
synthesizes an initial pre-loop load:

```
LoadInst *InitLoaded = Builder.CreateAlignedLoad(ResultTy, Addr, AddrAlign);
```

It then calls `InitLoaded->setVolatile(IsVolatile)` and conditionally
`InitLoaded->setAtomic(Monotonic, SSID)`. The subsequent cmpxchg (created via
the `CreateCmpXchg` callback) goes through `createCmpXchgInstFun` which
explicitly calls `copyMetadataForAtomic(*Pair, *MetadataSrc)` at line 758 to
carry `!tbaa`, `!alias.scope`, `!noalias`, `!noalias.addrspace`,
`!access_group`, `!mmra` onto the cmpxchg.

The `InitLoaded` seed load does NOT receive any of that metadata. It only
gets the metadata collected by `ReplacementIRBuilder` (which is just
`!pcsections` and `!annotation`, see ctor at line 64).

The omission is invisible in a peephole reading of the file because the
copy is on the cmpxchg, not the load. But the InitLoaded participates in the
SAME memory location and aliasing semantics as the cmpxchg and the original
atomicrmw.

## Trigger condition (x86)
`X86TargetLowering::shouldExpandAtomicRMWInIR` returns `CmpXChg` for
`Nand`, `Max`, `Min`, `UMax`, `UMin`, `FAdd`, `FSub`, `FMax`, `FMin`,
`UIncWrap`, `UDecWrap`, `USubCond`, `USubSat` (see X86ISelLowering.cpp:32979-32995).
These RMW ops always require a cmpxchg loop on x86, regardless of size.

Reproducer:

```
target triple = "x86_64-unknown-linux-gnu"

define i32 @test_tbaa_nand(ptr %p) {
  %r = atomicrmw nand ptr %p, i32 1 seq_cst, align 4, !tbaa !2, !noalias !6
  ret i32 %r
}

define i32 @test_tbaa_uincwrap(ptr %p) {
  %r = atomicrmw uinc_wrap ptr %p, i32 7 seq_cst, align 4, !tbaa !2, !noalias !6
  ret i32 %r
}

!0 = !{!"alias-domain"}
!1 = !{!"alias-scope-a", !0}
!2 = !{!3, !3, i64 0}
!3 = !{!"int", !4, i64 0}
!4 = !{!"omnipotent char", !5, i64 0}
!5 = !{!"Simple C/C++ TBAA"}
!6 = !{!1}
```

After `llc -mtriple=x86_64-unknown-linux-gnu -stop-after=atomic-expand`:

```
define i32 @test_tbaa_nand(ptr %p) {
  %1 = load i32, ptr %p, align 4                       ; <-- BUG: no !tbaa, no !noalias
  br label %atomicrmw.start
atomicrmw.start:
  %loaded = phi i32 [ %1, %0 ], [ %newloaded, %atomicrmw.start ]
  %2 = and i32 %loaded, 1
  %new = xor i32 %2, -1
  %3 = cmpxchg ptr %p, i32 %loaded, i32 %new seq_cst seq_cst, align 4, !tbaa !0, !noalias !4
  %success = extractvalue { i32, i1 } %3, 1
  %newloaded = extractvalue { i32, i1 } %3, 0
  br i1 %success, label %atomicrmw.end, label %atomicrmw.start
atomicrmw.end:
  ret i32 %newloaded
}

define i32 @test_tbaa_uincwrap(ptr %p) {
  %1 = load i32, ptr %p, align 4                       ; <-- BUG: no !tbaa, no !noalias
  br label %atomicrmw.start
  ...
}
```

`%1` (the InitLoaded seed) lacks `!tbaa` and `!noalias` while the cmpxchg
in the same loop carries both.

## Why this is a miscompile (not a quality issue)
After AtomicExpand, the IR is handed back to the codegen pipeline and to the
DAG combiner. The `!noalias` metadata on the cmpxchg promises the compiler
that the cmpxchg's memory access does not alias the noalias-scope-tagged
pointers in scope `!1`. But the SAME pointer `%p` is now accessed by an
untagged load. A later pass that ingests the IR (e.g. a LICM/CSE under a
PGSO/LTO recompile, or a manual `opt -tbaa -mldst-motion` run after this
file is dumped) may:

1. CSE/merge the untagged `%1 = load i32, ptr %p` with a TBAA-tagged load of
   a different alias type at `ptr %p` (e.g. `load i32 ... !tbaa <other-type>`)
   that the original atomicrmw with TBAA `!tbaa !2` was explicitly distinct
   from. CSE looks for syntactic equality of the address; with no TBAA on the
   InitLoaded, the metadata-strip semantics of `MDNode::getMostGenericTBAA`
   make this load identical to any other untagged i32 load at `%p` even
   across alias domains.
2. Use `BasicAA + TBAA` to conclude an aliasing store BEFORE the InitLoaded
   does NOT alias (because the cmpxchg with TBAA `!tbaa !2` says so), then
   sink the store past it; but for the InitLoaded the AA query collapses to
   plain BasicAA which DOES report MayAlias, producing inconsistent
   reorderings between the seed load and the cmpxchg in the loop. The two
   accesses to `%p` are now treated differently by aliasing analysis even
   though they are the SAME atomicrmw decomposition.
3. With `!noalias !6`, an interloping load tagged `!alias.scope !6` MUST
   not be moved across the original RMW. After expansion the cmpxchg still
   forbids the move, but the InitLoaded does not, so the interloping load
   may now be sunk between the InitLoaded and the cmpxchg loop -- a
   semantic move that crosses the *original* RMW boundary.

This is the same class of metadata-loss miscompile as #088
(`w88-rmwcmpxchgloop-initload-drops-md.md`) but for the more impactful
TBAA/noalias/alias.scope metadata rather than `pcsections`.

## Fix
At line 1769, after creating `InitLoaded`, call
`copyMetadataForAtomic(*InitLoaded, *MetadataSrc)` (with the `MetadataSrc`
argument that the function already receives at line 1740). The cmpxchg in
the loop already does this at line 758; the seed load must too.

## Related bugs
- #088 (`w88-rmwcmpxchgloop-initload-drops-md.md`): same call site, focuses
  on `!pcsections`; this entry covers the more dangerous AA metadata.
- #066: convertAtomicLoadToIntegerType / expandAtomicLoadToCmpXchg drop
  syncscope/volatile -- sibling call sites in same file.
- #024: widenPartwordAtomicRMW drops volatile.

The fix in #088 (if applied) does not necessarily cover this case because
the metadata families are different switch arms in `copyMetadataForAtomic`.
