# w95: InstCombine `combineLoadToOperationType` / `combineLoadToNewType` drops `!invariant.group` when retyping a load

## Files

- `llvm/lib/Transforms/InstCombine/InstCombineLoadStoreAlloca.cpp`
  - `InstCombinerImpl::combineLoadToNewType` at line 600
  - `combineLoadToOperationType` at line 686 (calls `combineLoadToNewType` on lines 718, 747, 792)
- `llvm/lib/Transforms/Utils/Local.cpp`
  - `llvm::copyMetadataForLoad` at line 3119

## Root cause

`combineLoadToNewType` builds the new `LoadInst` and then delegates metadata
propagation to `copyMetadataForLoad` (Local.cpp:3119). That switch handles
many MD kinds (`!tbaa`, `!range`, `!invariant.load`, `!nontemporal`, etc.) but
**does not contain a `case LLVMContext::MD_invariant_group`**, so the metadata
falls through the `default` of `getAllMetadata`-iteration with no action and is
silently dropped on the new load. Compare with `combineMetadata` in the same
file at line 2934 which *does* enumerate `MD_invariant_group` (line 2994) and
takes explicit action to preserve it.

Relevant excerpt from `copyMetadataForLoad`:

```cpp
switch (ID) {
case LLVMContext::MD_dbg:
case LLVMContext::MD_tbaa:
...
case LLVMContext::MD_invariant_load:
...
case LLVMContext::MD_nofpclass:
  ...
}
// NOTE: no case for MD_invariant_group, no default fallthrough that copies
```

## Concrete IR (reproduces against the local build)

```llvm
define ptr @load_ig_asc(ptr %p) {
  %v = load ptr, ptr %p, align 8, !invariant.group !0
  %c = bitcast ptr %v to ptr   ; same-type bitcast classifies as noop cast
  ret ptr %c
}
!0 = !{!"vt"}
```

`build/llvm-fuzzer/bin/opt -passes=instcombine -S`:

```llvm
define ptr @load_ig_asc(ptr %p) {
  %v1 = load ptr, ptr %p, align 8
  ret ptr %v1
}
```

`!invariant.group` is gone from the resulting load. Same loss reproduces with a
non-pointer bitcast retype:

```llvm
define i32 @load_ig(ptr %p) {
  %v = load float, ptr %p, align 4, !invariant.group !0
  %r = bitcast float %v to i32
  ret i32 %r
}
!0 = !{!"some-group"}
```

after `-passes=instcombine`:

```llvm
define i32 @load_ig(ptr %p) {
  %v1 = load i32, ptr %p, align 4
  ret i32 %v1
}
```

## Miscompile angle

`!invariant.group` is a correctness-relevant metadata used by devirtualization
of C++ vtable accesses. Two loads `load ptr p, !invariant.group !{}` from the
same pointer with no `launder.invariant.group` between them are guaranteed by
LangRef to return the same value. If InstCombine drops the metadata when
retyping one of the loads (e.g. because of an intervening same-type bitcast or
because of a small canonical type change introduced by a different pass), a
later `GVN` / `Devirtualization` / `LICM` pass loses the equivalence proof and
fails to fold. The resulting program is still type-safe, but the optimization
that allowed turning an indirect call into a direct one no longer fires.

When `-fstrict-vtable-pointers` is used, code patterns intentionally rely on
this metadata for correctness on Itanium-style invariant.group memory models,
so silently dropping the metadata can change observable behavior of the
optimization sequence (a stale vtable read across what the user expected to be
an invariant pair). The fix is a one-liner: add `case
LLVMContext::MD_invariant_group: Dest.setMetadata(ID, N); break;` to the switch
in `copyMetadataForLoad`, mirroring `combineMetadata`'s explicit handling.

## Confidence

High that the metadata is dropped (verified by reproducer above).
Medium that this is a true miscompile vs. a missed-optimization: in isolation
this only loses information, but in interaction with devirtualization /
strict-vtable-pointers pipelines it can change which calls are direct vs.
indirect, which is observable.
