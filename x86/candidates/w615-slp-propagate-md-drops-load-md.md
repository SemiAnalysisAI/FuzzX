# w615: SLP combined loads silently drop `!align`, `!nonnull`, `!dereferenceable`, `!noundef`, `!range`

## Class
Info-loss / optimization-quality regression (not a miscompile). Combined vector
loads emitted by SLPVectorizer lose pointer/value attribute metadata that the
scalar loads carried, blocking downstream alias/value-range and alignment-based
optimizations on the combined load.

## Component
`llvm/lib/Transforms/Vectorize/SLPVectorizer.cpp` (calls)
`llvm/lib/Analysis/VectorUtils.cpp` (allowlist root cause)

## Source

`llvm/lib/Analysis/VectorUtils.cpp:1049-1068`:

```cpp
void llvm::getMetadataToPropagate(
    Instruction *Inst,
    SmallVectorImpl<std::pair<unsigned, MDNode *>> &Metadata) {
  Inst->getAllMetadataOtherThanDebugLoc(Metadata);
  static const unsigned SupportedIDs[] = {
      LLVMContext::MD_tbaa,         LLVMContext::MD_alias_scope,
      LLVMContext::MD_noalias,      LLVMContext::MD_fpmath,
      LLVMContext::MD_nontemporal,  LLVMContext::MD_invariant_load,
      LLVMContext::MD_access_group, LLVMContext::MD_mmra};
  // Remove any unsupported metadata kinds from Metadata.
  for (unsigned Idx = 0; Idx != Metadata.size();) {
    if (is_contained(SupportedIDs, Metadata[Idx].first)) { ++Idx; }
    else { std::swap(Metadata[Idx], Metadata.back()); Metadata.pop_back(); }
  }
}
```

The allowlist explicitly excludes `MD_range`, `MD_nonnull`, `MD_align`,
`MD_dereferenceable`, `MD_dereferenceable_or_null`, `MD_noundef`,
`MD_invariant_group`. Caller in SLP at `SLPVectorizer.cpp:23236-23238`:

```cpp
Value *V = E->State == TreeEntry::CompressVectorize
               ? NewLI
               : ::propagateMetadata(NewLI, E->Scalars);
```

For 4 scalar loads of `ptr` all carrying `!align !{i64 16}`, `!nonnull`,
`!dereferenceable !{i64 8}`, the combined `<4 x ptr>` load is emitted with
none of these attributes — even though the natural conservative combine
(intersection) would have preserved them in this case.

## Repro

```ll
; w615.ll  --  see also /tmp/slphunt/loadalign.ll
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

define void @t(ptr noalias %src, ptr noalias %dst) {
entry:
  %p0 = getelementptr inbounds ptr, ptr %src, i64 0
  %p1 = getelementptr inbounds ptr, ptr %src, i64 1
  %p2 = getelementptr inbounds ptr, ptr %src, i64 2
  %p3 = getelementptr inbounds ptr, ptr %src, i64 3
  %l0 = load ptr, ptr %p0, align 8, !align !0, !nonnull !1, !dereferenceable !2
  %l1 = load ptr, ptr %p1, align 8, !align !0, !nonnull !1, !dereferenceable !2
  %l2 = load ptr, ptr %p2, align 8, !align !0, !nonnull !1, !dereferenceable !2
  %l3 = load ptr, ptr %p3, align 8, !align !0, !nonnull !1, !dereferenceable !2
  %q0 = getelementptr inbounds ptr, ptr %dst, i64 0
  %q1 = getelementptr inbounds ptr, ptr %dst, i64 1
  %q2 = getelementptr inbounds ptr, ptr %dst, i64 2
  %q3 = getelementptr inbounds ptr, ptr %dst, i64 3
  store ptr %l0, ptr %q0, align 8
  store ptr %l1, ptr %q1, align 8
  store ptr %l2, ptr %q2, align 8
  store ptr %l3, ptr %q3, align 8
  ret void
}
!0 = !{i64 16}
!1 = !{}
!2 = !{i64 8}
```

`opt -passes=slp-vectorizer -S w615.ll` produces:

```
  %0 = load <2 x ptr>, ptr %p0, align 8       ; !align/!nonnull/!dereferenceable dropped
  store <2 x ptr> %0, ptr %q0, align 8
  %1 = load <2 x ptr>, ptr %p2, align 8       ; dropped here too
  store <2 x ptr> %1, ptr %q2, align 8
```

A similar test with i32 loads bearing `!range !{i32 0, i32 100}, !noundef !{}`
(see `/tmp/slphunt/loadmd.ll`) shows both `!range` and `!noundef` are stripped
from the resulting `<4 x i32>` load.

## Why this matters
- Downstream alias analysis can no longer rely on the combined load returning a
  nonnull/aligned/dereferenceable pointer, blocking LICM hoisting, GVN, and
  loop-aware optimizations that would otherwise fire on the (still-valid)
  invariants.
- `!range` and `!noundef` are key inputs for known-bits/range propagation, so
  vectorization actively pessimizes code in subsequent passes.

Conservative direction: dropping is safe. But the fix is trivial — extend the
allowlist with `MD_range`, `MD_nonnull`, `MD_align`, `MD_dereferenceable`,
`MD_dereferenceable_or_null`, `MD_noundef`, `MD_invariant_group` and add the
matching combine ops in `propagateMetadata` (most reduce to `MDNode::intersect`
or `getMostGenericAlign`/getMostGenericRange-style helpers, several of which
already exist in `MDNode`).

## Severity
Low: pure optimization-quality loss (not miscompile). High prevalence: every
SLP'd load that carries pointer-attribute or value-range metadata is affected.

## Triage
Regression scope: long-standing — predates any of the recent SLP refactors.
