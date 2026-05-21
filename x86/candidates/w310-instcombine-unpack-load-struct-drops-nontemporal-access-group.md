# w310: InstCombine `unpackLoadToAggregate` STRUCT branch (multi-element) drops `!nontemporal`, `!access_group`, `!mem_parallel_loop_access`, `!noundef`, `!invariant.group`

## File / function

`llvm/lib/Transforms/InstCombine/InstCombineLoadStoreAlloca.cpp`,
`unpackLoadToAggregate` STRUCT branch with `NumElements > 1`,
lines 768-785.

## Root cause

The multi-element struct unpack synthesizes one new `LoadInst` per element
(line 773). The only metadata it copies onto each element load is:

```cpp
auto *L = IC.Builder.CreateAlignedLoad(...);
// Propagate AA metadata. It'll still be valid on the narrowed load.
L->setAAMetadata(LI.getAAMetadata());
// Copy invariant metadata from parent load.
L->copyMetadata(LI, LLVMContext::MD_invariant_load);
V = IC.Builder.CreateInsertValue(V, L, i);
```

`setAAMetadata` only handles `{tbaa, tbaa_struct, alias_scope, noalias}`
(see `Instruction::setAAMetadata`). Together with the explicit
`MD_invariant_load` copy, the only kinds preserved are those five plus
debug. Every other metadata kind that LLVM defines as
"directly applies after a type-preserving load split" is silently dropped:

- `!nontemporal`
- `!access_group`
- `!mem_parallel_loop_access`
- `!noundef`
- `!invariant.group`
- `!noalias_addrspace`

Compare with `copyMetadataForLoad` in `llvm/lib/Transforms/Utils/Local.cpp:3119`
which explicitly enumerates and propagates the first five of these on the
single-element struct fast-path (line 747, via `combineLoadToNewType`).

This is the SAME class of bug as w95 (which covered the ARRAY branch
dropping `!invariant.load`), but applied to the STRUCT branch and to a
larger set of metadata kinds. The struct branch is the one that *added*
the `copyMetadata(MD_invariant_load)` call but forgot every other
member of the "directly applies" set.

## Reproducer (built `opt`, default x86 `-O2`-level instcombine)

```llvm
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

%S = type { i32, i32 }

define %S @f(ptr %p) {
  %v = load %S, ptr %p, align 4,
      !nontemporal !0,
      !mem_parallel_loop_access !1,
      !access_group !2,
      !noundef !3
  ret %S %v
}

!0 = !{i32 1}
!1 = !{!2}
!2 = distinct !{}
!3 = !{}
```

### `opt -passes=instcombine -S` produces

```llvm
define %S @f(ptr %p) {
  %v.unpack = load i32, ptr %p, align 4                 ; <-- ALL metadata gone
  %1 = insertvalue %S poison, i32 %v.unpack, 0
  %v.elt1 = getelementptr inbounds nuw i8, ptr %p, i64 4
  %v.unpack2 = load i32, ptr %v.elt1, align 4           ; <-- ALL metadata gone
  %v3 = insertvalue %S %1, i32 %v.unpack2, 1
  ret %S %v3
}
```

Confirmed against `/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt`.

`!invariant.group` is also lost; minimal repro:

```llvm
define %S @f(ptr %p) {
  %v = load %S, ptr %p, align 4, !invariant.group !0
  ret %S %v
}
!0 = !{}
```
becomes
```llvm
  %v.unpack = load i32, ptr %p, align 4
  ...
  %v.unpack2 = load i32, %v.elt1, align 4               ; !invariant.group gone
```

## Why it matters

- `!nontemporal` is a backend codegen hint - losing it on a struct load
  silently turns a streaming load into a regular load, regressing the
  cache behavior the frontend asked for.
- `!access_group` / `!mem_parallel_loop_access` directly affect *correctness
  of subsequent passes*: LoopVectorize / LoopAccessAnalysis use these to
  decide that two memory ops may be reordered. Dropping the group token on
  half of the loads in a struct load means subsequent passes will treat
  these element loads as not vectorizable / not parallel.
- `!invariant.group` loss is the same vptr-substitution miscompile class
  documented in w106; the unpack path is just another route to it.
- `!noundef` loss blocks downstream poison-aware folds and can silently
  enable replacing the load with a constant that the frontend marked as
  guaranteed-no-undef.

## Fix shape

Two viable fixes:

1. Replace the ad-hoc `setAAMetadata` + `copyMetadata(MD_invariant_load)`
   pair with a `copyMetadataForLoad(*L, LI)` call (mirrors what
   `combineLoadToNewType` already does for the 1-element case).
   `copyMetadataForLoad` already excludes the pointer-only metadata
   (`align`, `dereferenceable`, etc.) when the new type is non-pointer.
2. Apply the same fix to the ARRAY branch on line 825 (covered by w95)
   and to the STRUCT 1-element fast-path on line 749 (which currently
   already goes through `copyMetadataForLoad` and is fine).

A more aggressive fix would also add `MD_invariant_group` to the
`copyMetadataForLoad` switch since that helper currently drops it too
(separate bug, see w95-instcombine-load-retype-drops-invariant-group).

## Confidence

High (verified by reproducer).
Distinct from w95 (which is the ARRAY branch + invariant_load only).
Distinct from w106 (which is the BitCast-unwrap path through
combineStoreToNewValue, not the unpack-aggregate path).
