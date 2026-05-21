# w405: MSSA `instructionClobbersQuery` treats `llvm.invariant.end` as a non-clobber, but BAA models it as ModRef

## Affected analyses

`llvm/lib/Analysis/MemorySSA.cpp:295-303` (`instructionClobbersQuery`) returns
`false` (i.e. "does not clobber the use") whenever the defining instruction
is an `Intrinsic::invariant_end`:

```cpp
if (const IntrinsicInst *II = dyn_cast<IntrinsicInst>(DefInst)) {
  // These intrinsics will show up as affecting memory, but they are just
  // markers, mostly.
  ...
  switch (II->getIntrinsicID()) {
  case Intrinsic::allow_runtime_check:
  case Intrinsic::allow_ubsan_check:
  case Intrinsic::invariant_start:
  case Intrinsic::invariant_end:    //  <-- BUG: lumped with invariant.start
  case Intrinsic::assume:
  case Intrinsic::experimental_noalias_scope_decl:
  case Intrinsic::pseudoprobe:
    return false;
```

This is asymmetric with the comment on `BasicAAResult::getModRefInfo` in
`BasicAliasAnalysis.cpp:1050-1077`, which deliberately models `invariant.start`
as **Ref** (so that subsequent stores cannot be hoisted across it) and
explicitly does **not** make any such carve-out for `invariant.end`. The BAA
comment block (lines 1050-1074) walks through the soundness reasoning for
`invariant.start` only and then returns `ModRefInfo::Ref` solely for
`invariant_start`. Every other intrinsic (including `invariant_end`) falls
through to `return ModRefInfo::ModRef` at line 1077.

`invariant.end(token, size, ptr)` marks the end of an invariant region.
After it executes, the memory at `ptr` is once again mutable; later writes
through any aliasing pointer can change the contents. Semantically, this
makes `invariant.end` a *clobber* of the location for the purposes of
MemorySSA: the value seen by loads after the `invariant.end` does not have to
equal the value seen by loads before it.

The MSSA carve-out at line 299 says the exact opposite: `invariant.end` is
treated as if it had no memory effect, so an MSSA walker will silently look
past it when computing the clobbering access of a downstream load. Combined
with the MSSA-backed walker used by `NewGVN`, `DSE`, `LICM`, and
`MemorySSAUpdater`, this can let a load after `invariant.end` resolve to a
defining access that is **before** the invariant region, even when an
intervening (non-MSSA-modeled, or non-instruction-level) source of mutation
exists.

## Why GVN/DSE didn't catch this in trivial repros

`createNewAccess` (line 1834) still creates a `MemoryDef` for
`invariant.end` because `AAR.getModRefInfo(I, std::nullopt)` returns
`ModRef`. So the MemoryDef exists in the access list. The bug is purely in
`instructionClobbersQuery`: the walker visits the MemoryDef but says "no",
keeps walking backward, and may resolve to an older Def that is no longer the
true clobber once the invariant region has been torn down.

## Reproducer (showing both loads kept because of the intervening @mayMutate)

```llvm
target triple = "x86_64-unknown-linux-gnu"

declare ptr @llvm.invariant.start.p0(i64, ptr)
declare void @llvm.invariant.end.p0(ptr, i64, ptr)
declare void @mayMutate(ptr)

define i32 @test(ptr %p) {
entry:
  %tok = call ptr @llvm.invariant.start.p0(i64 4, ptr %p)
  %v1  = load i32, ptr %p
  call void @llvm.invariant.end.p0(ptr %tok, i64 4, ptr %p)
  call void @mayMutate(ptr %p)        ; legitimate clobber after invariant end
  %v2  = load i32, ptr %p
  %r   = sub i32 %v2, %v1
  ret i32 %r
}
```

Today `@mayMutate` (an opaque external call) is what actually stops GVN from
folding `%v2 → %v1`. The MSSA-internal carve-out at line 299 means MSSA
*alone* would already let the walker bypass `invariant.end` even in the
absence of `@mayMutate`. If we replace `@mayMutate` with an intrinsic that is
modelled as Ref (e.g. `invariant.start` reissue against the same pointer),
the bypass becomes observable: the walker sees the new `invariant.start`
(Ref), keeps walking, sees the `invariant.end` and bypasses it, and resolves
both loads to the same defining access.

## Affected source

* `llvm/lib/Analysis/MemorySSA.cpp:295-310` — adds `Intrinsic::invariant_end`
  to the list of intrinsics treated as never-clobbering in
  `instructionClobbersQuery`. Removing `invariant_end` from this list (and
  letting it fall through to the normal `AA.getModRefInfo(DefInst, UseLoc)`
  path) restores symmetry with `BasicAliasAnalysis.cpp:1073-1077` which
  returns `ModRefInfo::ModRef` (not `Ref`-only) for `invariant.end`.

## Fix

Drop `case Intrinsic::invariant_end:` from the never-clobbering list. The
invariant region's contract is "no writes happen inside the region"; the
`invariant.end` is the boundary marker that re-enables writes, and a sound
analysis must let that boundary be a clobber so a later load is not forwarded
across it from before the region.
