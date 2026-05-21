# 228 — GVN `eliminatePartiallyRedundantLoad` (PRE) drops `!nonnull`, `!dereferenceable`, `!align`, `!noundef`, `!nontemporal`, `!fpmath`

Component: `llvm/lib/Transforms/Scalar/GVN.cpp` lines ~1565-1604 (PRE-inserted load metadata-copy whitelist)

The PRE path inserts a hoisted load and hand-rolls a metadata-copy whitelist that omits several load-applicable metadata kinds:
- `!nonnull` (pointer can be assumed non-null)
- `!dereferenceable` (load-result pointer derefenceable byte-count)
- `!align` (load-result pointer alignment guarantee)
- `!noundef`
- `!nontemporal` (cache hint)
- `!fpmath` (FP load precision)

Different from local CSE which uses `combineMetadataForCSE` — PRE has its own whitelist that's stricter than load-safe set.

## Reproducer

`opt -passes=gvn -S repro.ll`

Input merge-block load carries all 5 kinds. After GVN, the PRE-inserted `%v.pre` in `then` has zero metadata; the post-merge phi cannot reconstruct it.

## Severity

Default x86 -O2. Per `w580` verification: also reproduces at default `-O2`.

## Fix

Replace hand-rolled whitelist with `copyMetadataForLoad` from `Utils/Local.cpp` which already enumerates the load-safe kinds.
