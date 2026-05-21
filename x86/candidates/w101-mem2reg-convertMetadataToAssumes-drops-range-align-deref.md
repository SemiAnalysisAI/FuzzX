# w101 mem2reg: `convertMetadataToAssumes` only preserves `!nonnull`/`!noundef`, silently drops `!range`, `!align`, `!dereferenceable`, `!dereferenceable_or_null`

## Location

- `llvm/lib/Transforms/Utils/PromoteMemoryToRegister.cpp:500-520`
  (`convertMetadataToAssumes`)
- Callers: lines 620, 727, 1195 (single-store, single-block,
  multi-block PHI rewriting paths).

## Bug

When mem2reg replaces a `load` with the corresponding stored SSA value,
it strips the load instruction and, with it, every piece of
load-attached metadata. To compensate, mem2reg defines
`convertMetadataToAssumes`, which is supposed to re-express the load's
metadata-encoded UB / refinement contract as `llvm.assume` calls (or a
non-terminator unreachable for `!noundef`):

```cpp
static void convertMetadataToAssumes(LoadInst *LI, Value *Val,
                                     const DataLayout &DL, AssumptionCache *AC,
                                     const DominatorTree *DT) {
  if (isa<UndefValue>(Val) && LI->hasMetadata(LLVMContext::MD_noundef)) {
    // Insert non-terminator unreachable.
    LLVMContext &Ctx = LI->getContext();
    new StoreInst(ConstantInt::getTrue(Ctx),
                  PoisonValue::get(PointerType::getUnqual(Ctx)),
                  /*isVolatile=*/false, Align(1), LI->getIterator());
    return;
  }

  // If the load was marked as nonnull ...
  if (AC && LI->getMetadata(LLVMContext::MD_nonnull) &&
      LI->getMetadata(LLVMContext::MD_noundef) &&
      !isKnownNonZero(Val, SimplifyQuery(DL, DT, AC, LI)))
    addAssumeNonNull(AC, LI);
}
```

The function only handles two cases:

1. `Val` is a literal `UndefValue` AND load had `!noundef`
   -> inject unreachable.
2. Load had BOTH `!nonnull` AND `!noundef` AND `Val` is not provably
   non-zero -> insert `assume(Val != null)`.

Every other load-attached metadata that carries a hint or a
poison-on-violation contract is silently discarded:

- `!range` (poison on out-of-range value)
- `!align` (poison on misaligned pointer)
- `!dereferenceable` (poison on insufficient deref)
- `!dereferenceable_or_null`
- `!noundef` *without* `!nonnull` (when the store value is not literally
  `UndefValue`)
- `!nonnull` *without* `!noundef` (intentionally guarded above)
- `!nontemporal`, `!invariant.load`, `!invariant.group`
- `!fpmath`, `!alias.scope`, `!noalias`, `!tbaa` (the AAMetadata trio)

For the hint-only flavors (`!range`, `!align`, `!dereferenceable`,
`!fpmath`) this is a missed-optimization, not a miscompile: downstream
passes (InstCombine, SCEV, codegen alignment-aware lowering) lose the
information the frontend deliberately attached.

For the contract-bearing flavors (`!noundef` without `!nonnull`,
`!invariant.load`) this is more sharply an optimization-power loss
because the immediate-UB-on-violation contract used to be available.

## Reproducers

`!range` is silently dropped:

```ll
; mem2reg.range.ll
define i32 @load_range_dropped(i32 %arg) {
entry:
  %a = alloca i32, align 4
  store i32 %arg, ptr %a, align 4
  %v = load i32, ptr %a, align 4, !range !0
  ret i32 %v
}
!0 = !{i32 0, i32 10}
```

```text
$ opt -passes='mem2reg' mem2reg.range.ll -S
define i32 @load_range_dropped(i32 %arg) {
entry:
  ret i32 %arg
}
```

Before mem2reg, an IR consumer can see that the *load result* is in
`[0, 10)`. After mem2reg, that fact is gone — no `assume(... < 10)` is
inserted, no metadata is forwarded to a use, the value is just `%arg`.

`!dereferenceable`:

```ll
define ptr @load_deref_dropped(ptr %p) {
entry:
  %a = alloca ptr, align 8
  store ptr %p, ptr %a, align 8
  %v = load ptr, ptr %a, align 8, !dereferenceable !0
  ret ptr %v
}
!0 = !{i64 64}
```

```text
$ opt -passes='mem2reg' mem2reg.deref.ll -S
define ptr @load_deref_dropped(ptr %p) {
entry:
  ret ptr %p
}
```

The frontend-supplied 64-byte deref window is dropped; subsequent
hoisting/speculation queries cannot recover it.

`!align`:

```ll
define ptr @load_align_dropped(ptr %p) {
entry:
  %a = alloca ptr, align 8
  store ptr %p, ptr %a, align 8
  %v = load ptr, ptr %a, align 8, !align !0
  ret ptr %v
}
!0 = !{i64 32}
```

```text
$ opt -passes='mem2reg' mem2reg.align.ll -S
define ptr @load_align_dropped(ptr %p) {
entry:
  ret ptr %p
}
```

The 32-byte alignment fact on the load result is dropped.

## Why this matters even though metadata is "just hints"

`!range`, `!align`, and `!dereferenceable` are documented as
poison-on-violation. After mem2reg, the value carrying these guarantees
is no longer load-attached, so it cannot be expressed as IR metadata at
all on an SSA value. The only escape hatch the codebase already uses is
`llvm.assume` — and `convertMetadataToAssumes` deliberately uses that
hatch, but only for `!nonnull`. Three obvious sibling cases (`!range`
-> `assume(v u< Hi)` + `assume(v u>= Lo)`, `!align` -> `assume((iptr v)
& (A-1) == 0)`, `!dereferenceable` -> a `dereferenceable` parameter
attribute on a freshly-conjured no-op intrinsic call) are missing.

This loses optimization power in any pass that lowers an
LLVMContext::MD\_\* metadata kind into known-bits/known-align/known-deref
state — e.g., InstCombine's alignment refinement, BasicAA's
dereferenceable reasoning, codegen's load-hoist-with-deref-attr.

## Fix sketch

Extend `convertMetadataToAssumes` to also emit:

- `!range !{Lo, Hi}` -> `assume(icmp uge Val, Lo)` and
  `assume(icmp ult Val, Hi)` (wrap-around case requires the OR form).
- `!align !{Align}` (and the optional `!align !{Align, Offset}`) ->
  `assume(icmp eq, and (ptrtoint Val), Mask, Offset)`.
- `!dereferenceable !{N}` -> `assume(Val)` with
  `OperandBundle("dereferenceable", N)`.
- `!nonnull` alone (no `!noundef`) is already documented as
  "deliberately not propagated"; keep it.

Each of these has precedent in `Local.cpp`'s `combineMetadataForCSE`
where the same kinds are already merged across two loads.

## Notes

- Pure IR-level: no codegen reproducer.
- `sroa` exhibits the same drop (it ultimately calls into the same
  promotion logic via `PromoteMemToReg`).
- Pre-existing analogues in the candidates pool:
  `w83-gvn-pre-drops-loadinst-metadata.md` and
  `w42-simplifycfg-hoistcondloads-drops-pointer-metadata.md` flag the
  same pattern in different passes.
