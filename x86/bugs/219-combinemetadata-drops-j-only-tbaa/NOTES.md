# 219 — `combineMetadata` iterates only K's metadata; J-only kinds silently dropped during CSE

Component: `llvm/lib/Transforms/Utils/Local.cpp` line 2937 (`K->getAllMetadataOtherThanDebugLoc(Metadata)`)

`combineMetadata(K, J, ...)` only iterates the metadata kinds present on `K`. Anything attached to `J` but not `K` never reaches the per-kind switch and is silently dropped.

In particular: when EarlyCSE/GVN/SimplifyCFG/MergedLoadStoreMotion CSEs `J` (which has `!tbaa`) against an earlier `K` (which has none), the merged value (K replaces J's uses) ends up with no TBAA — even though one of the two operands had a tag.

## Reproducer

```ll
%a = load i32, ptr %p, align 4, !tbaa !0   ; has TBAA
%b = load i32, ptr %p, align 4              ; no TBAA
%s = add i32 %a, %b
```

`opt -passes='early-cse<memssa>' -S` produces:
```
%a = load i32, ptr %p, align 4, !tbaa !0
%s = add i32 %a, %a     ; both refs go to %a; TBAA preserved
```

But when the order is reversed (the load WITHOUT TBAA comes first as the dominator), EarlyCSE keeps `%a` (no TBAA) and drops `%b`. The replacement value has no TBAA at all — `%b`'s TBAA was J-only and never iterated.

## Severity

Default x86 -O2. Loses downstream AA precision. Subtle because it depends on which of the two equivalent loads dominates.

## Fix

Walk the union of K's and J's metadata, not just K's. (Or iterate J's separately for kinds K doesn't have.)
