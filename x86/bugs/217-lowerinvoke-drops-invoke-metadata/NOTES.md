# 217 — `LowerInvoke` invoke→call rewrite drops `!prof`, `!annotation`, `!range`, `!callees`, `!nosanitize`, `!noalias`, `!alias.scope`

Component: `llvm/lib/Transforms/Utils/LowerInvoke.cpp` lines ~53-58

When `invoke` is rewritten to `call`, the new `CallInst` is built without `copyMetadata(...)`. All invoke-attached metadata is silently dropped. The new unconditional `br` to the continuation block also has no `setDebugLoc`.

## Reproducer

`opt -passes=lower-invoke -S repro.ll` — output `%r = call i32 @callee()` has none of `!prof`, `!annotation`, `!range`.

## Severity

`lower-invoke` is not in the default x86 -O2 pipeline (most LLVM users keep invokes through codegen), but is reachable in EH-stripped builds (e.g., GPU codegen, certain WASM and embedded toolchains). Documented here as a real opt-diff bug — non-default pipeline.

## Fix

`NewCall->copyMetadata(*II);` after building the call.
