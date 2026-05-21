# w685: MemorySSA `MemoryLocOrCall(FenceInst)` leaves union `Loc` uninitialized

Source: `llvm/lib/Analysis/MemorySSA.cpp`
Anonymous-namespace class `MemoryLocOrCall`, defined at line 159, has a
private union of `const CallBase *Call` and `MemoryLocation Loc`. The
`Instruction *` constructor leaves the union *un-initialized* when the
instruction is a fence:

```cpp
// MemorySSA.cpp:168
MemoryLocOrCall(Instruction *Inst) {
  if (auto *C = dyn_cast<CallBase>(Inst)) {
    IsCall = true;
    Call = C;
  } else {
    IsCall = false;
    // There is no such thing as a memorylocation for a fence inst, and it is
    // unique in that regard.
    if (!isa<FenceInst>(Inst))            // <-- fence: skip Loc init
      Loc = MemoryLocation::get(Inst);
  }
}
```

For a fence the `else` branch is entered (so `IsCall` is correctly `false`)
but the `Loc` union member is never assigned. Because `union { CallBase
*; MemoryLocation; }` has no implicit member life-time (and the union has
no constructor), reading `Loc` for a fence-constructed `MemoryLocOrCall`
is undefined behavior.

This object can subsequently flow through:

  * `DenseMap<MemoryLocOrCall, MemlocStackInfo>::operator[]` (line 1400) –
    the densemap will hash via
    `DenseMapInfo<MemoryLocOrCall>::getHashValue`, which dispatches on
    `IsCall`; for `!IsCall` it dereferences `.getLoc()` (line 232 ->
    line 188 `assert(!IsCall); return Loc;`).
  * `operator==` (line 193) – also dereferences `Loc`.

The current callers happen to use `MemoryLocOrCall` only with
`MemoryUse` instances (`optimizeUsesInBlock` line 1399) — and a fence is
modelled as a `MemoryDef`, never a `MemoryUse`. So in the present code
the uninitialized branch is not reached. The danger is *latent*: any
future caller that constructs `MemoryLocOrCall(MemoryUseOrDef *)` from a
fence-backed Def (e.g. `MemorySSAUtil::defClobbersUseOrDef(Def, Def)`
overload, or any new analysis that keys a map by Def-backed locations)
will hash UB-stamped garbage.

There is also one production path that already crosses the boundary:
`MemorySSAUtil::defClobbersUseOrDef(MemoryDef *MD, const MemoryUseOrDef
*MU, AAResults &AA)` (line 340) calls `MemoryLocOrCall(MU)`. Today every
in-tree caller (`GVNHoist.cpp:611`) passes a `MemoryUse *` for `MU`, but
the API signature accepts any `MemoryUseOrDef *`. A future caller that
passes a fence-backed `MemoryDef` here would tickle the UB.

## Why this is the right fix surface, not just a stylistic issue

The author's intent (see comment "There is no such thing as a memorylocation
for a fence inst") is to treat fence as a sentinel. The correct
implementation should either:

1. Assert that `IsCall || !isa<FenceInst>(Inst)` at construction (forcing
   callers to never build a `MemoryLocOrCall` from a fence), **or**
2. Initialize `Loc` to a sentinel (`Loc = MemoryLocation()`) and have
   `operator==` / `getHashValue` treat that sentinel correctly.

Either fix removes the latent UB without changing behaviour for the
present callers.

## Downstream-miscompile reproducer

I was **not** able to construct a downstream miscompile (the latent path
is currently guarded by all callers using `MemoryUse` only). Random
fuzzing 1000+ programs at `-O2` and through `loop-mssa(licm,
simple-loop-unswitch)`, `memcpyopt`, `dse`, `gvn-hoist`, all combined
with `verify<memoryssa>`, did not surface a miscompile or assertion.

This entry is filed as a **source-only finding** rather than a confirmed
miscompile so the bug-list captures the source defect; please demote /
re-classify if the project prefers reproducer-gated entries.

## Additional MemorySSA construction-time defect, related

The MemoryLocOrCall union-initialization issue *would have downstream
impact* if combined with the comment at MemorySSA.cpp:1693:

```cpp
// Note that moving should implicitly invalidate the optimized state of a
// MemoryUse (and Phis can't be optimized). However, it doesn't do so for a
// MemoryDef.
if (auto *MD = dyn_cast<MemoryDef>(What))
  MD->resetOptimized();
What->setBlock(BB);
```

The comment says it *doesn't* do that for a MemoryDef, but the code below
the comment *does* call `resetOptimized()` on the MemoryDef. The comment
is stale. Not a miscompile, but a documentation/contract bug worth
flagging while in the area.

## Files inspected (absolute paths)

* `/home/orenamd@semianalysis.com/FuzzX/amdgpu/third_party/llvm-project/llvm/lib/Analysis/MemorySSA.cpp`
  * Class `MemoryLocOrCall`, lines 159–213
  * `instructionClobbersQuery`, lines 280–337
  * `defClobbersUseOrDef`, lines 340–343
  * `createNewAccess`, lines 1768–1846
  * `OptimizeUses::optimizeUsesInBlock`, lines 1364–1499
* `/home/orenamd@semianalysis.com/FuzzX/amdgpu/third_party/llvm-project/llvm/lib/Analysis/MemorySSAUpdater.cpp`
  * `cloneUsesAndDefs`, lines 589–619
  * `moveAllAccesses`, lines 1193–1226
  * `moveAllAfterMergeBlocks`, lines 1239–1247
* `/home/orenamd@semianalysis.com/FuzzX/amdgpu/third_party/llvm-project/llvm/lib/Analysis/AliasAnalysis.cpp`
  * `getSyncEffects`, lines 461–485
  * `AAResults::getModRefInfo(FenceInst*, …)`, lines 542–559
