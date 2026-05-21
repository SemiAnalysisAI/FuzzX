# w639: `expandPartwordCmpXchg` drops AA metadata on synthesized `cmpxchg` and `load`

## Severity
Alias-analysis miscompile risk. Same class as w636, but in the partword
path. Reachable on any target with `MinCmpXchgSize > byte/halfword` (AMDGPU,
RISC-V without Zabha, LoongArch base ISA, MIPS, SPARC, Hexagon, VE, Xtensa,
PowerPC without partword atomics). Reachable from the X86 build with
`-mtriple=amdgcn-amd-amdhsa`.

## Source

`llvm/lib/CodeGen/AtomicExpandPass.cpp:1153-1289`

```cpp
bool AtomicExpandImpl::expandPartwordCmpXchg(AtomicCmpXchgInst *CI) {
  ...
  // Load the entire current word, and mask into place the expected and new
  // values
  LoadInst *InitLoaded = Builder.CreateLoad(PMV.WordType, PMV.AlignedAddr);  // line 1221
  // <-- no copyMetadataForAtomic(*InitLoaded, *CI)
  ...
  AtomicCmpXchgInst *NewCI = Builder.CreateAtomicCmpXchg(
      PMV.AlignedAddr, FullWord_Cmp, FullWord_NewVal, PMV.AlignedAddrAlignment,
      CI->getSuccessOrdering(), CI->getFailureOrdering(), CI->getSyncScopeID()); // line 1247
  NewCI->setVolatile(CI->isVolatile());                                          // line 1250
  NewCI->setWeak(CI->isWeak());                                                  // line 1256
  // <-- no copyMetadataForAtomic(*NewCI, *CI)
  ...
}
```

Compare with the sister `widenPartwordAtomicRMW` at lines 1141-1145, which
*does* call `copyMetadataForAtomic(*NewAI, *AI)` after creating the wide
atomicrmw. Same comment in
`AtomicExpandPass.cpp:232-263` (the `copyMetadataForAtomic` helper's filter
list - `tbaa`, `tbaa_struct`, `alias_scope`, `noalias`, `noalias_addrspace`,
`access_group`, `mmra`, `dbg`, plus the AMDGPU-specific
`amdgpu.no.remote.memory` / `amdgpu.no.fine.grained.memory`).

The `InitLoaded` load (line 1221) is also synthesized without any metadata
propagation. It's later upgraded to atomic when
`shouldIssueAtomicLoadForAtomicEmulationLoop()` is true (line 1236-1242),
but that upgrade also doesn't add metadata.

## Repro - partword i8 cmpxchg on AMDGPU loses `!tbaa`

```llvm
target triple = "amdgcn-amd-amdhsa"

define { i8, i1 } @cas_i8_tbaa(ptr %p, i8 %c, i8 %n) {
  %r = cmpxchg ptr %p, i8 %c, i8 %n seq_cst seq_cst, align 1, !tbaa !0
  ret { i8, i1 } %r
}

!0 = !{!1, !1, i64 0}
!1 = !{!"char", !2, i64 0}
!2 = !{!"omnipotent char", !3, i64 0}
!3 = !{!"Simple C++ TBAA"}
```

```console
$ llc -mtriple=amdgcn-amd-amdhsa -mcpu=gfx900 -stop-after=atomic-expand repro.ll -o -
define { i8, i1 } @cas_i8_tbaa(ptr %p, i8 %c, i8 %n) #0 {
  %AlignedAddr = call ptr @llvm.ptrmask.p0.i64(ptr %p, i64 -4)
  ...
  %7 = load i32, ptr %AlignedAddr, align 4                       ; !tbaa lost
  ...
partword.cmpxchg.loop:
  ...
  %12 = cmpxchg ptr %AlignedAddr, i32 %11, i32 %10 seq_cst seq_cst, align 4   ; !tbaa lost
  ...
}
```

Both the initial load (`%7`) and the loop cmpxchg (`%12`) lack the
`!tbaa !0` that the source `cmpxchg` carried.

## Cross-reference

Same root cause as w636/w638: every callsite in `AtomicExpandPass.cpp` that
synthesizes a fresh load/store/cmpxchg has to remember which subset of
source-instruction metadata to propagate. `expandPartwordCmpXchg` forgets
both the initial load and the loop cmpxchg.

## Suggested fix

```cpp
LoadInst *InitLoaded = Builder.CreateLoad(PMV.WordType, PMV.AlignedAddr);
copyMetadataForAtomic(*InitLoaded, *CI);
...
NewCI->setWeak(CI->isWeak());
copyMetadataForAtomic(*NewCI, *CI);
```

Also worth auditing `expandAtomicCmpXchg` (`AtomicExpandPass.cpp:1451-1699`)
- it synthesizes load-linked / store-conditional pairs via `TLI->emitLoadLinked`
which is fully target-controlled, so metadata propagation there has to be
handled by each target's TLI hook implementation. That's out of scope for
this candidate but follows the same diagnostic pattern.

## opt/llc diff

- `opt`: not affected by default (atomic-expand isn't in the default pipeline).
- `llc -stop-after=atomic-expand` on AMDGPU (or any partword-CAS target):
  produced IR has no `!tbaa` / `!alias.scope` / `!noalias` on the rewritten
  load or cmpxchg. Final assembly may be correct in isolation; the hazard is
  for any later IR-level pass that runs after AtomicExpand and consults AA.
