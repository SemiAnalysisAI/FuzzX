# 244 — `SCEVExpander::expandAddToGEP` drops `inrange`, `inbounds`, and `nusw` on synthesized GEPs

Component: `llvm/lib/Transforms/Utils/ScalarEvolutionExpander.cpp` lines ~397-399, 436

`expandAddToGEP` builds new GEPs via `Builder.CreatePtrAdd(...)` which routes to `Constant::getGetElementPtr(..., InRange=std::nullopt)`. The GEP flags arm at lines 392-394 only sets `noUnsignedWrap` when `FlagNUW` is set — never `noUnsignedSignedWrap` (despite a dedicated factory), never `inBounds()`, never `inrange(...)`.

## Reproducer

`opt -passes=indvars -S repro.ll`

Source has `getelementptr inbounds inrange(-8, 24) (...)`. After indvars, `%scevgep = getelementptr i8, ptr @vt, i64 %0` — both `inbounds` and `inrange(-8, 24)` silently lost.

## Severity

Default x86 -O2 — indvars/LSR canonicalization commonly synthesizes GEPs and loses these attributes. Downstream alias analysis and bounds-check passes lose the IR-level guarantees.

## Fix

In `expandAddToGEP`, propagate `inbounds`/`nusw` from the source GEP's flags, and pass the source's `inrange` to `CreatePtrAdd`.
