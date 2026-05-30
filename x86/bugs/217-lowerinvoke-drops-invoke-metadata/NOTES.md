# 217 — `LowerInvoke` invoke→call rewrite drops `!prof`, `!annotation`, `!range`, `!callees`, `!nosanitize`, `!noalias`, `!alias.scope`

Component: `llvm/lib/Transforms/Utils/LowerInvoke.cpp` lines ~53-58

When `invoke` is rewritten to `call`, the new `CallInst` is built without `copyMetadata(...)`. All invoke-attached metadata is silently dropped. The new unconditional `br` to the continuation block also has no `setDebugLoc`.

## Reproducer

`opt -passes=lower-invoke -S repro.ll` — output `%r = call i32 @callee()` has none of `!prof`, `!annotation`, `!range`.

## Severity

`lower-invoke` is not in the default x86 -O2 pipeline (most LLVM users keep invokes through codegen), but is reachable in EH-stripped builds (e.g., GPU codegen, certain WASM and embedded toolchains). Documented here as a real opt-diff bug — non-default pipeline.

## Fix

`NewCall->copyMetadata(*II);` after building the call.

## WONTFIX

Two reasons:

1. **Non-default pass.** `lower-invoke` is not in the default x86 -O2/-O3 pipeline; most toolchains carry invokes through codegen. It's reachable only on EH-stripping paths (some GPU/WASM/embedded flows), so the opt-diff is not observable in the mainline pipeline.

2. **The naive one-liner is itself buggy.** `copyMetadata(*II)` copies *all* metadata, including `!prof`. An `invoke`'s `branch_weights` may have two operands (normal + unwind), but a `call`'s must have exactly one — the verifier enforces `ExpectedNumOperands = 1` for `CallInst` (`Verifier.cpp`). Blindly copying the two-weight `!0 = !{!"branch_weights", i32 100, i32 1}` in this very repro produces IR the verifier rejects ("Wrong number of operands"). The correct conversion already exists as `llvm::createCallMatchingInvoke` (`Local.cpp:2594`), which copies metadata and *then* collapses `!prof` to the single total weight via `extractProfTotalWeight`. LowerInvoke's whole rewrite loop is essentially `llvm::changeToCall(II)`.

A partial reimplementation isn't worth carrying for a non-default pass. Branch `fix6-lowerinvoke-copymetadata` dropped.
