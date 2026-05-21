# 200 — MemCpyOpt `processStoreOfLoad` drops load's `!nontemporal` (and AAMD) into synthesized memcpy

Component: `llvm/lib/Transforms/Scalar/MemCpyOptimizer.cpp` lines 631-702 (`processStoreOfLoad`)

When `load %T; store %T` is replaced with `memcpy`/`memmove`, the only propagation done is `M->copyMetadata(*SI, LLVMContext::MD_DIAssignID)` (line 685). There is no `combineAAMetadata(M, LI)` or `combineAAMetadata(M, SI)`, and `MD_nontemporal`/`MD_invariant_load` from the load are silently dropped. Sister function `processByValArgument` (line 2070) correctly calls `combineAAMetadata`; this path does not.

`!nontemporal` lost on the load means the resulting memcpy lowers to plain mov instructions instead of MOVNT-class loads, reverting an explicit cache hint set by the user.

## Reproducer

`opt -passes=memcpyopt -S repro.ll` — the synthesized `llvm.memcpy` has no `!nontemporal`.

## Severity

Default x86 -O2. Fires on the very common load-aggregate-then-store-aggregate pattern.
