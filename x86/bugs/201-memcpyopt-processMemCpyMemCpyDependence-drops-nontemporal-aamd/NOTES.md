# 201 — MemCpyOpt `processMemCpyMemCpyDependence` drops the source memcpy's `!nontemporal` (and AAMD)

Component: `llvm/lib/Transforms/Scalar/MemCpyOptimizer.cpp` line 1256

The chained-memcpy fold replaces `memcpy(mid,src) + memcpy(dst,mid)` with `memcpy(dst,src)`. The new intrinsic does not call `combineAAMetadata(NewM, M)` or `combineAAMetadata(NewM, MDep)`, so `!alias.scope`/`!noalias`/`!tbaa`/`!nontemporal` from the source memcpy disappear.

Verified: the outer memcpy initially carries `!nontemporal !0`; after `opt -passes=memcpyopt -S` the surviving memcpy has no metadata.

## Severity

Default x86 -O2. Memory-ordering / cache-hint loss in standard memcpy chain.

## Fix

After the new memcpy is built, call `combineAAMetadata(NewM, M); combineAAMetadata(NewM, MDep);` and propagate `MD_nontemporal` from either source.
