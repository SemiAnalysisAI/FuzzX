# w406: `MemoryDependenceAnalysis::GetLocation` has no case for `memcpy_element_unordered_atomic`, falling back to coarse ModRef

## Affected analysis

`llvm/lib/Analysis/MemoryDependenceAnalysis.cpp:153-187`
(`static ModRefInfo GetLocation(const Instruction *, MemoryLocation &, ...)`).

The switch enumerates `lifetime_start`, `lifetime_end`, `invariant_start`,
`invariant_end`, `masked_load`, `masked_store`. It has **no** case for the
unordered-atomic memory intrinsics:

* `Intrinsic::memcpy_element_unordered_atomic`
* `Intrinsic::memmove_element_unordered_atomic`
* `Intrinsic::memset_element_unordered_atomic`

After the switch (line 182-187) the fallback is:

```cpp
// Otherwise, just do the coarse-grained thing that always works.
if (Inst->mayWriteToMemory())
  return ModRefInfo::ModRef;
if (Inst->mayReadFromMemory())
  return ModRefInfo::Ref;
```

So an atomic-memcpy returns `ModRefInfo::ModRef` (because it both reads and
writes) **with `Loc.Ptr == nullptr`**. The caller in
`getCallDependencyFrom` (line 207-235) then treats `Loc.Ptr == nullptr` as
the "no per-argument location available" path and uses the coarse modref
result against the entire query — never asking BAA whether the atomic
memcpy's destination is disjoint from the query.

This is **less precise** than the non-atomic `Intrinsic::memcpy` case, which
in `MemoryLocation::getForArgument` (lines 194-206) does build a precise
per-argument location for the atomic variant. So the precision is available;
it just isn't wired into MDA's `GetLocation`.

## Why it matters

`MemoryDependenceAnalysis` is still used today by:

* `GVN` (`llvm/lib/Transforms/Scalar/GVN.cpp`) — primary user
* `MemCpyOptimizer`
* The DSE legacy path

For all of them, a call to MDA's `getCallDependencyFrom` against an atomic
memcpy currently returns "Clobber" against any other memory access in the
block (because the coarse modref is ModRef and there is no `Loc.Ptr` to
disambiguate). This stops valid forwarding/elimination opportunities that
*would* succeed if MDA queried BAA per-argument the way the non-atomic case
does.

This is a **missed optimization**, not a miscompile. But because MDA is
shared between transforms, the *opposite* asymmetry — a transform that has
already used `MemoryLocation::getForSource` / `getForDest` on the atomic
memcpy and forwarded a more aggressive answer — can then receive a
contradictory "Clobber" from MDA, causing pass-pipeline thrash and
fragile-equality issues for VN passes that assume MDA and BAA agree.

## Reduced reproducer (no miscompile; precision regression)

```llvm
target triple = "x86_64-unknown-linux-gnu"

declare void @llvm.memcpy.element.unordered.atomic.p0.p0.i64(ptr, ptr, i64, i32)

define void @test(ptr noalias %src, ptr noalias %dst, ptr noalias %other) {
entry:
  store i32 42, ptr %other
  call void @llvm.memcpy.element.unordered.atomic.p0.p0.i64(
      ptr align 4 %dst, ptr align 4 %src, i64 16, i32 4)
  store i32 99, ptr %other
  ret void
}
```

With pipeline `-passes=gvn`, the atomic memcpy is forwarded through (today
its modref against `%other` is computed by BAA per-arg, so GVN succeeds).
With pipeline that exercises MDA's `getCallDependencyFrom` (e.g. when the
atomic memcpy itself is the query, looking for a previous identical call),
MDA's per-arg refinement never runs.

## Affected source

* `llvm/lib/Analysis/MemoryDependenceAnalysis.cpp:153-180` (the switch)
* Inconsistency vs `llvm/lib/Analysis/MemoryLocation.cpp:198-200` (which DOES
  handle the atomic memcpy variants when asked via `getForArgument`)

## Fix

Add the three `*_element_unordered_atomic` intrinsics to the switch in
`MemoryDependenceAnalysis::GetLocation`, mirroring the non-atomic memcpy/memset
handling (set `Loc` via `MemoryLocation::getForArgument` for the dest, return
`ModRefInfo::Mod`/`Ref`/`ModRef` as appropriate).
