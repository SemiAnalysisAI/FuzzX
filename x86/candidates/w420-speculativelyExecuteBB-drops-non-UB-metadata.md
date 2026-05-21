# w420 — `speculativelyExecuteBB` drops `!tbaa`, `!nontemporal`, `!alias.scope`, `!noalias`, `!access_group`, `!mem_parallel_loop_access`, `!invariant.load`, `!invariant.group`

Severity: missed optimization (loss of AA / vectorization / cache-hint metadata). Not a miscompile.

## Where

`llvm/lib/Transforms/Utils/SimplifyCFG.cpp:3386`

```cpp
// Metadata can be dependent on the condition we are hoisting above.
// Strip all UB-implying metadata on the instruction. Drop the debug loc
// to avoid making it appear as if the condition is a constant, which would
// be misleading while debugging.
// Similarly strip attributes that maybe dependent on condition we are
// hoisting above.
for (auto &I : make_early_inc_range(*ThenBB)) {
  if (!SpeculatedStoreValue || &I != SpeculatedStore) {
    I.dropLocation();
  }
  I.dropUBImplyingAttrsAndMetadata();   // <-- this line
  ...
}
```

Helper definition: `llvm/lib/IR/Instruction.cpp:586`

```cpp
void Instruction::dropUBImplyingAttrsAndMetadata(ArrayRef<unsigned> Keep) {
  static const unsigned KnownIDs[] = {
      LLVMContext::MD_annotation, LLVMContext::MD_range,
      LLVMContext::MD_nonnull,    LLVMContext::MD_align,
      LLVMContext::MD_fpmath,     LLVMContext::MD_prof};
  ...
}
```

## What's wrong

`speculativelyExecuteBB` hoists at most one non-cheap instruction from a then-block above its condition. Before splicing it up, every instruction in the then-block has `dropUBImplyingAttrsAndMetadata()` called on it, which keeps only `!annotation`, `!range`, `!nonnull`, `!align`, `!fpmath`, `!prof`. Everything else is silently dropped, including:

- `!tbaa` — pure AA hint, not UB-implying
- `!nontemporal` — cache hint, not UB-implying
- `!invariant.load` — caller-asserts-load-is-invariant, safe to hoist
- `!invariant.group` — vtable-load identity hint, safe to hoist
- `!access_group`, `!mem_parallel_loop_access` — loop vectorizer scaffolding
- `!callees`, `!callee_type`, `!callsite`, `!memprof` — IPO hints

`!alias.scope` and `!noalias` are also dropped here; that is arguably correct
because the noalias claim could be condition-dependent. But the bulk of the
list (`!tbaa`, `!nontemporal`, `!invariant.load`, `!invariant.group`,
`!access_group`, `!mem_parallel_loop_access`) is unrelated to the dominating
condition and need not be stripped on hoisting.

The comment in the source ("Strip all UB-implying metadata") is misleading —
the helper drops *all metadata except* a tiny allowlist, not just UB-implying
kinds.

## Severity / class

Loss of optimization metadata. Concrete downstream impacts:

- Loss of `!tbaa` here can prevent later AA-based DSE/LICM from disambiguating loads.
- Loss of `!invariant.load` blocks GVN/MemoryDependence from treating the speculated load as invariant.
- Loss of `!access_group`/`!mem_parallel_loop_access` can demote a parallel loop back to serial.

Not a miscompile of program behavior.

## Reproducer

`/tmp/w420/t36_spec_tbaa_dropped.ll`:

```ll
target datalayout = "e-m:e-p:64:64-i64:64-v128:128:128-a:0:64-S64"
target triple = "x86_64-unknown-linux-gnu"

declare i32 @llvm.smin.i32(i32, i32) nounwind readnone willreturn

; The CFG shape is what triggers speculativelyExecuteBB rather than
; foldTwoEntryPHINode: %end has 3 preds, %then has 1 pred and unconditionally
; jumps to %end, so %then is speculated above its enclosing branch.
define i32 @f(i1 %c1, i1 %c2, i32 %x) {
entry:
  br i1 %c1, label %end, label %check
check:
  br i1 %c2, label %end, label %then
then:
  %v = call i32 @llvm.smin.i32(i32 %x, i32 100), !tbaa !0
  br label %end
end:
  %r = phi i32 [ 0, %entry ], [ 1, %check ], [ %v, %then ]
  ret i32 %r
}

!0 = !{!1, !1, i64 0}
!1 = !{!"int", !2, i64 0}
!2 = !{!"omnipotent char", !3, i64 0}
!3 = !{!"Simple C/C++ TBAA"}
```

Pipeline confirmed default: `opt -passes=simplifycfg -S`. No non-default
SimplifyCFG option needed — `speculativelyExecuteBB` is reached from the
default `simplifyCondBranch` path (`SimplifyCFG.cpp:8652` / `:8661`).

After `opt -passes=simplifycfg -S`:

```ll
define i32 @f(i1 %c1, i1 %c2, i32 %x) {
entry:
  %brmerge = select i1 %c1, i1 true, i1 %c2
  %.mux = select i1 %c1, i32 0, i32 1
  %v = call i32 @llvm.smin.i32(i32 %x, i32 100)        ; !tbaa GONE
  %r = select i1 %brmerge, i32 %.mux, i32 %v
  ret i32 %r
}
```

Same shape with `!alias.scope` / `!noalias` / `!annotation`: only `!annotation`
survives (the others are dropped). See `/tmp/w420/t32_spec_alias_scope.ll`.

## Notes

- Same root cause manifests via `hoistAllInstructionsInto` (`llvm/lib/Transforms/Utils/Local.cpp:3421`), which `foldTwoEntryPHINode` calls — see candidate w421.
- Suggested fix: expand the keep-list in `dropUBImplyingAttrsAndMetadata` to
  include `MD_tbaa`, `MD_nontemporal`, `MD_invariant_load`, `MD_invariant_group`,
  `MD_access_group`, `MD_mem_parallel_loop_access`, `MD_memprof`, `MD_callsite`,
  `MD_callees`, `MD_callee_type`. Alternatively, plumb a `Keep` list at each
  call site that knows which metadata kinds are safe for the specific transform.
- Test files at `/tmp/w420/t31_*.ll` through `/tmp/w420/t36_*.ll` exercise the
  speculation paths.
