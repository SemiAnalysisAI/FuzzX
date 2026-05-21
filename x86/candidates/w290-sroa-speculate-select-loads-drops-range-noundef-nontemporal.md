# w290 -- SROA `speculateSelectInstLoads` drops `!range`, `!noundef`, `!nontemporal` on the speculated loads (and only AA tags survive)

## Component
`llvm/lib/Transforms/Scalar/SROA.cpp`, `speculateSelectInstLoads` at lines
1767-1803.

## Root cause
When SROA speculates a `load (select c, p1, p2)` into
`select c, (load p1), (load p2)`, the only metadata copied from the original
load to the two new loads is `AAMDNodes` (line 1791-1795):

```cpp
AAMDNodes Tags = LI.getAAMetadata();
if (Tags) {
  TL->setAAMetadata(Tags);
  FL->setAAMetadata(Tags);
}
```

Every other piece of load metadata is silently dropped. In particular:

- `!range` -- a value-range guarantee that downstream passes (InstCombine,
  GVN, CVP, CodeGen) rely on for folding.
- `!noundef` -- a freeze-removal/poison-reasoning hint that other passes use to
  prove non-undef.
- `!nontemporal` -- a CodeGen hint that controls non-temporal load lowering
  (e.g. `MOVNTDQA`); silently losing it changes generated x86 code.
- `!invariant.load`, `!invariant.group`, `!align`, `!dereferenceable*`,
  `!alias.scope`, `!noalias` etc. -- all dropped.

Compare to `llvm::copyMetadataForLoad` in Local.cpp:3119-3177 which is the
designated helper for copying load metadata when only the type changes.
That helper preserves all of the above and is what should be used (or an
equivalent open-coded list).

The sibling speculation routine `speculatePHINodeLoads` at line 1624 has the
same issue (only AA tags + alignment), but covered by w97. The select path is
not covered by any prior candidate (w106 is the *InstCombine* load-of-select
fold, which uses `Metadata::PoisonGeneratingIDs` — a different but adjacent
bug). The SROA select path is a separate file/function/line; the offset is
that SROA copies *only* AA, while InstCombine copies *only* PoisonGenerating.

## Reproducer

```ll
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

define i32 @test(i1 %c, ptr nonnull align 4 dereferenceable(4) %ext) {
entry:
  %a = alloca i32, align 4
  store i32 0, ptr %a, align 4
  %p = select i1 %c, ptr %a, ptr %ext
  %v = load i32, ptr %p, align 4, !range !0, !noundef !1, !nontemporal !2
  %r = and i32 %v, 1
  ret i32 %r
}

!0 = !{i32 0, i32 1}
!1 = !{}
!2 = !{i32 1}
```

`opt -passes=sroa -S` (x86 -O2 default sroa is preserve-cfg, this triggers
`speculateSelectInstLoads` because both arms are safe to speculate):

```ll
define i32 @test(i1 %c, ptr nonnull align 4 dereferenceable(4) %ext) {
entry:
  %v.sroa.speculate.load.false = load i32, ptr %ext, align 4
  %v.sroa.speculated = select i1 %c, i32 0, i32 %v.sroa.speculate.load.false
  %r = and i32 %v.sroa.speculated, 1
  ret i32 %r
}
```

No `!range`, no `!noundef`, no `!nontemporal` on the speculated load.

## Downstream impact (missed-opt)

Running `opt -passes='sroa,instcombine' -S`:
```ll
define i32 @test(i1 %c, ptr nonnull align 4 dereferenceable(4) %ext) {
entry:
  %v.sroa.speculate.load.false = load i32, ptr %ext, align 4
  %0 = and i32 %v.sroa.speculate.load.false, 1
  %r = select i1 %c, i32 0, i32 %0
  ret i32 %r
}
```

With the original `!range !{i32 0, i32 1}`, the source-level program can be
folded to a single `and`+`select` that further simplifies (the range proves
the high bits are zero so `and ..., 1` becomes the value itself, and the
true-arm `0 & 1 = 0`). After SROA, `!range` is gone and the constant-prop chain
is broken on the false arm. The `!nontemporal` loss also changes generated
x86 code (loss of streaming hint for the speculative load).

For `!nontemporal` the loss is a documented codegen regression: the front-end
asked for a non-temporal load and the back-end now emits a regular load.

## Fix sketch
Replace the AA-only copy at SROA.cpp:1791-1795 with `copyMetadataForLoad(*TL,
LI); copyMetadataForLoad(*FL, LI);` (or an equivalent list of
load-safe metadata), and only override AA tags afterwards if needed.

## Notes
- Default x86 -O2 only. Confirmed on LLVM 23.0.0git (FuzzX `opt` build).
- Distinct from w97 (`speculatePHINodeLoads` AA/align), distinct from w106
  (InstCombine load-of-select PoisonGeneratingIDs), distinct from w95
  (InstCombine load-of-select drops `!noundef`/`!invariant.load`).
