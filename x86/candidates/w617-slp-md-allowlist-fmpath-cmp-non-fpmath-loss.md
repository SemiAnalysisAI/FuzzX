# w617: SLP propagateMetadata allowlist does NOT include `!invariant.group`

## Class
Info-loss (not miscompile). Combined loads/stores lose `!invariant.group`,
defeating the devirtualization invariant the metadata is designed to preserve.

## Component
`llvm/lib/Analysis/VectorUtils.cpp:1049-1068` (`getMetadataToPropagate`)
called from
`llvm/lib/Transforms/Vectorize/SLPVectorizer.cpp:23238` (loads),
`llvm/lib/Transforms/Vectorize/SLPVectorizer.cpp:23294` (stores).

## Source

```cpp
// llvm/lib/Analysis/VectorUtils.cpp:1053
static const unsigned SupportedIDs[] = {
    LLVMContext::MD_tbaa,         LLVMContext::MD_alias_scope,
    LLVMContext::MD_noalias,      LLVMContext::MD_fpmath,
    LLVMContext::MD_nontemporal,  LLVMContext::MD_invariant_load,
    LLVMContext::MD_access_group, LLVMContext::MD_mmra};
```

`MD_invariant_group` is missing. LangRef defines `!invariant.group` as a
key handle for devirtualization: loads with matching `!invariant.group` are
known to produce the same value regardless of intervening barriers/calls. SLP
combining 4 such loads into a `<4 x ptr>` load silently sheds that property.

## Repro

```ll
; w617.ll
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

%vt = type { ptr }

define void @t(ptr noalias %src, ptr noalias %dst) {
entry:
  %p0 = getelementptr inbounds ptr, ptr %src, i64 0
  %p1 = getelementptr inbounds ptr, ptr %src, i64 1
  %p2 = getelementptr inbounds ptr, ptr %src, i64 2
  %p3 = getelementptr inbounds ptr, ptr %src, i64 3
  %l0 = load ptr, ptr %p0, align 8, !invariant.group !0
  %l1 = load ptr, ptr %p1, align 8, !invariant.group !0
  %l2 = load ptr, ptr %p2, align 8, !invariant.group !0
  %l3 = load ptr, ptr %p3, align 8, !invariant.group !0
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

!0 = !{!"vt"}
```

`opt -passes=slp-vectorizer -S w617.ll` emits a `<2 x ptr>` (or `<4 x ptr>`)
load with no `!invariant.group`. Subsequent `gvn` / `inline-deferral` /
`devirt` cannot reuse the original invariant relation.

## Severity / Triage
Low. Single-line fix: add `MD_invariant_group` to the allowlist and `intersect`
it (same node when all match, nullptr otherwise — `MDNode::intersect` already
gives the right behavior because all matching nodes are uniqued by content).

## Cross-ref
Same root cause as w615/w616. Filed separately because the optimization
impact (devirtualization through `inline-deferral` / `gvn`) is qualitatively
different from numeric-range / pointer-alignment regressions.
