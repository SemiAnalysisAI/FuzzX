# w407: `BasicAAResult::getModRefInfo(Call1, Call2)` only special-cases `experimental_guard`; atomic intrinsics fall to coarse ModRef

## Affected analysis

`llvm/lib/Analysis/BasicAliasAnalysis.cpp:1080-1106`
(`ModRefInfo BasicAAResult::getModRefInfo(const CallBase *Call1, const CallBase *Call2, AAQueryInfo &AAQI)`)

The Call-vs-Call entry point in BAA looks at `Call1` and `Call2` only through
the lens of `Intrinsic::experimental_guard`. Every other intrinsic — including
the *unordered-atomic memcpy family* and `Intrinsic::masked_load` /
`Intrinsic::masked_store` — receives the conservative answer `ModRefInfo::ModRef`
at line 1105 (`// Be conservative.`).

```cpp
if (isIntrinsicCall(Call1, Intrinsic::experimental_guard))
  return isModSet(getMemoryEffects(Call2, AAQI).getModRef())
             ? ModRefInfo::Ref
             : ModRefInfo::NoModRef;

if (isIntrinsicCall(Call2, Intrinsic::experimental_guard))
  return isModSet(getMemoryEffects(Call1, AAQI).getModRef())
             ? ModRefInfo::Mod
             : ModRefInfo::NoModRef;

// Be conservative.
return ModRefInfo::ModRef;
```

In contrast, the per-arg refinement loop in `BasicAAResult::getModRefInfo(Call,
Loc, AAQI)` at lines 998-1021 *does* break down a call's arg modref against a
concrete memory location, even for the atomic memcpy variants. That logic is
only reachable in the Call-vs-Loc path.

## Why it matters

When MSSA's `optimizeUsesInBlock` (`MemorySSA.cpp:1478-1483`) walks upward
from a Use that is itself a `CallBase`, it ends up at
`instructionClobbersQuery(MD, MU, ...)` in `MemorySSA.cpp:313-316`, which
calls `AA.getModRefInfo(DefInst, CB)`. This is the Call-vs-Call entry point
above. So if either:

* the prior `MemoryDef` is an atomic-memcpy intrinsic; or
* the current `MemoryUse` (here passed as Call) is an atomic-memcpy intrinsic;

then BAA returns `ModRefInfo::ModRef` regardless of disjointness of the
buffers (it never consults `MemoryLocation::getForArgument` for either side).
MSSA then conservatively bails the optimization, producing a stricter
clobber chain than necessary.

Concretely, two `noalias`-disjoint atomic memcpys followed in sequence will
not be recognised as non-interfering at the Call-vs-Call entry point, even
though Call-vs-Loc would prove them disjoint.

## Reduced reproducer

```llvm
target triple = "x86_64-unknown-linux-gnu"

declare void @llvm.memcpy.element.unordered.atomic.p0.p0.i64(ptr, ptr, i64, i32)

define void @test(ptr noalias %a, ptr noalias %b, ptr noalias %c, ptr noalias %d) {
entry:
  call void @llvm.memcpy.element.unordered.atomic.p0.p0.i64(
      ptr align 4 %a, ptr align 4 %b, i64 16, i32 4)
  call void @llvm.memcpy.element.unordered.atomic.p0.p0.i64(
      ptr align 4 %c, ptr align 4 %d, i64 16, i32 4)
  ret void
}
```

`opt -passes=memoryssa -print-memoryssa` will show that the second atomic
memcpy's MemoryDef has the first as its defining access (i.e. they are
treated as potentially clobbering each other) even though all four pointers
are `noalias`. Replacing the atomic memcpys with non-atomic ones lets the
Call-vs-Loc refinement at `BasicAliasAnalysis.cpp:998-1021` see disjointness
and produce a tighter MSSA.

## Affected source

* `llvm/lib/Analysis/BasicAliasAnalysis.cpp:1080-1106` — Call-vs-Call entry
  point. Add an arg-based refinement similar to the Call-vs-Loc loop at
  lines 998-1021: if `Call1`'s effects are `argmem`-only, walk its pointer
  args, build `MemoryLocation::getForArgument(Call1, ArgIdx, TLI)`, and ask
  `AAQI.AAR.getModRefInfo(Call2, ArgLoc, AAQI)` for each. Combine.

## Fix

Lift the per-arg refinement from the Call-vs-Loc path into the Call-vs-Call
path. For each call, if `getMemoryEffects(Call).getModRef(ArgMem)` is set
and the other accesses (Other, ErrnoMem) are NoModRef, then the call only
touches its pointer args; the Call-vs-Call answer is then the join over
those per-arg `getModRefInfo(other_call, ArgLoc)` queries.
