# w409: `BasicAAResult::getModRefInfo` only adds sync effects for `isModAndRefSet(OtherMR)`, missing write-only asm-fences

## Affected analysis

`llvm/lib/Analysis/BasicAliasAnalysis.cpp:966-978`
(`BasicAAResult::getModRefInfo(const CallBase *Call, const MemoryLocation &Loc, AAQueryInfo &AAQI)`).

```cpp
// Take into account potential synchronization effects of the call.
// We assume synchronization can not occur if the call does not read/write
// other memory (this in particular ensures that readonly/argmemonly continue
// to work as expected for frontends that do not emit nosync).
// FIXME: This should apply to all calls, but is limited to inline asm to
// limit impact. ...
ModRefInfo SyncMR = ModRefInfo::NoModRef;
if (isModAndRefSet(OtherMR) && Call->maySynchronize() &&
    Call->isInlineAsm()) {
  SyncMR = getSyncEffects(&AAQI.AAR, Loc, AAQI);
  if (isModAndRefSet(SyncMR))
    return SyncMR;
}
```

The guard is `isModAndRefSet(OtherMR)` — synchronization is *only* injected
when the call's "other memory" effects are **both** Mod and Ref. That misses:

* Inline-asm sequences that **write-only** to `OtherMem` (e.g. `mfence`
  surrogates declared as `"=*m"`-only, plus `"memory"` clobber for
  ordering, plus `sideeffect` for non-removability).
* Inline-asm sequences that **read-only** from `OtherMem` (less common,
  but synchronisation-bearing reads exist: e.g. `lfence`-around-rdtsc
  expressed as a single read of a magic side-channel location).

For both cases, `OtherMR` is `Mod` or `Ref` (not both), so the
synchronization injection is skipped, and the inline asm is allowed to be
reordered with non-aliasing accesses **across the synchronization point**.

The intent expressed in the in-source comment ("limited to inline asm to
limit impact") is correct; the bug is the *additional* over-restriction to
`isModAndRefSet`. The real `getSyncEffects` already does its own filtering;
the outer guard should be `isModOrRefSet`, not `isModAndRefSet`, to keep
write-only and read-only fence-asm honored.

## Concrete failure pattern

```llvm
target triple = "x86_64-unknown-linux-gnu"

@flag = global i32 0

define void @producer(ptr %payload, i32 %v) {
  store i32 %v, ptr %payload
  ; "Write-only" mfence-as-inline-asm: argmem-none, OtherMem=Mod only
  call void asm sideeffect "mfence", "~{memory}"() #0
  store atomic i32 1, ptr @flag release, align 4
  ret void
}

attributes #0 = { nounwind willreturn memory(write) }
```

The inline `mfence` here is declared `memory(write)` — `OtherMR` resolves
to `Mod` only. `Call->maySynchronize()` is true. `isInlineAsm()` is true.
But `isModAndRefSet(OtherMR)` is false (only Mod, no Ref), so the sync-
effects path at lines 973-977 is skipped entirely.

The result: a later AA query that asks whether the inline asm clobbers
some non-`@flag` memory will see only `OtherMR=Mod`, with no synchronisation
floor. Downstream code (e.g. LICM, MachineSink reordering through MSSA
walker, GVN/DSE through MDA) may move a non-aliasing access across the
`mfence`, defeating the ordering the user actually wrote.

## Why this is non-trivial to demonstrate as a transform diff

Most frontends emit `memory(readwrite)` ("memory" clobber both reads and
writes other memory) when they intend a barrier; that hits the `isModAndRefSet`
branch and synchronisation is honored. Hand-written / specialised code that
narrows the asm memory model to write-only (or read-only) intentionally —
because they know exactly what the asm touches — silently loses the
synchronisation floor here.

## Affected source

* `llvm/lib/Analysis/BasicAliasAnalysis.cpp:973-978`

## Fix

Change the guard from `isModAndRefSet(OtherMR)` to `isModOrRefSet(OtherMR)`
so any non-`NoModRef` OtherMR with `maySynchronize() && isInlineAsm()`
participates in the `getSyncEffects` path. The second guard at line 976
(`isModAndRefSet(SyncMR)`) already gates whether the SyncMR is returned
early, so widening the outer guard does not over-restrict and only adds the
write-only-asm case that is currently missing.
