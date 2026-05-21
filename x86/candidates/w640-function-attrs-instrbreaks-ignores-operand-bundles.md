# w640: FunctionAttrs InstrBreaksNoFree/NoSync/NonThrowing ignore clobbering operand bundles, infers nofree/nosync/nounwind/willreturn on callsites that may free/sync/throw/diverge via the bundle

Files:
- `llvm/lib/Transforms/IPO/FunctionAttrs.cpp`
  - `InstrBreaksNoFree` lines 1919-1934
  - `InstrBreaksNoSync` lines 1936-1948
  - `InstrBreaksNonThrowing` lines 1903-1917
  - (related, but correct) `addMemoryAttrs` / `checkFunctionMemoryAccess` line 204:
    `if (!Call->hasOperandBundles() && Call->getCalledFunction() && SCCNodes.count(...))`
- `llvm/lib/IR/Instruction.cpp`
  - `Instruction::willReturn()` lines 1294-1302 (delegates to `CB->hasFnAttr(Attribute::WillReturn)`, also ignores opbundles)

## Root cause

`addMemoryAttrs` and `checkFunctionMemoryAccess` know that an operand bundle on a call
can introduce arbitrary additional read/write/clobber effects, and explicitly bail when
the call has operand bundles (FunctionAttrs.cpp:204):
```cpp
if (!Call->hasOperandBundles() && Call->getCalledFunction() &&
    SCCNodes.count(Call->getCalledFunction())) {
  ...continue (speculatively treat as no-op for this SCC iteration)...
}
```
After that check it falls through to merging in `AAR.getMemoryEffects(Call)`, which
*does* fold in `hasReadingOperandBundles()` / `hasClobberingOperandBundles()` via
`CallBase::getMemoryEffects()` (Instructions.cpp:632-650).

The three "instruction breaks attribute" helpers used by `inferAttrsFromFunctionBodies`
have no equivalent check. They look only at `CB->hasFnAttr(Attribute::NoFree/NoSync/NoUnwind)`
or at SCC membership of the called function:

```cpp
// FunctionAttrs.cpp:1920-1934
static bool InstrBreaksNoFree(Instruction &I, const SCCNodeSet &SCCNodes) {
  CallBase *CB = dyn_cast<CallBase>(&I);
  if (!CB)
    return false;
  if (CB->hasFnAttr(Attribute::NoFree))     // <-- ignores opbundles
    return false;
  if (Function *Callee = CB->getCalledFunction())
    if (SCCNodes.contains(Callee))          // <-- ignores opbundles
      return false;
  return true;
}

// FunctionAttrs.cpp:1936-1948
static bool InstrBreaksNoSync(Instruction &I, const SCCNodeSet &SCCNodes) {
  if (!I.maySynchronize())                  // <-- maySynchronize for Call = !hasFnAttr(NoSync)
    return false;
  if (auto *CB = dyn_cast<CallBase>(&I))
    if (Function *Callee = CB->getCalledFunction())
      if (SCCNodes.contains(Callee))        // <-- ignores opbundles
        return false;
  return true;
}

// FunctionAttrs.cpp:1903-1917
static bool InstrBreaksNonThrowing(Instruction &I, const SCCNodeSet &SCCNodes) {
  if (!I.mayThrow(/*IncludePhaseOneUnwind*/ true))  // <-- mayThrow ~ !doesNotThrow ~ !hasFnAttr(NoUnwind)
    return false;
  if (const auto *CI = dyn_cast<CallInst>(&I))
    if (Function *Callee = CI->getCalledFunction())
      if (SCCNodes.contains(Callee))                // <-- ignores opbundles
        return false;
  return true;
}
```

`CallBase::hasFnAttr` (InstrTypes.h:1460, impl line 2329) checks the call-site
AttributeList then forwards to `hasFnAttrOnCalledFunction`. Neither path consults
operand bundles. The only opbundle-aware fast path on a `CallBase` is `getMemoryEffects()`,
which is invoked by `addMemoryAttrs` but *not* by `InstrBreaks*`.

Per LangRef ("Operand Bundles"):

> An operand bundle ... may have arbitrary side effects, including reading from
> and writing to memory, depending on the bundle tag.

Generic / unknown-tag bundles are conservatively "clobbering" — see
`CallBase::hasClobberingOperandBundles` (Instructions.cpp:623-630), which only
whitelists `{ptrauth, kcfi, convergencectrl, deactivation_symbol, deopt, funclet}`.
Anything else (and any non-assume reading bundle) is treated as potentially
reading and clobbering for *memory* purposes — but the freeing-memory,
synchronizing, throwing and willreturn predicates have no equivalent
opbundle-aware override, so the bundle's worst-case behavior is invisible to
`InstrBreaks*`.

`Instruction::willReturn()` (Instruction.cpp:1294-1302) has the same shape:
```cpp
bool Instruction::willReturn() const {
  if (isVolatile()) return false;
  if (const auto *CB = dyn_cast<CallBase>(this))
    return CB->hasFnAttr(Attribute::WillReturn);     // <-- ignores opbundles
  return true;
}
```
`functionWillReturn` (FunctionAttrs.cpp:2207) walks every instruction with this
predicate, so `willreturn` is also unsoundly inferred when the call has an
arbitrary opbundle.

## Concrete IR (reproduces against the local opt @ /home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt)

```llvm
; test_repro_final.ll
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

declare void @leaf() nofree nosync nounwind willreturn

define void @caller() {
  call void @leaf() [ "side_effects"() ]
  ret void
}
```

`opt -passes=function-attrs -S`:
```llvm
; Function Attrs: nofree nosync nounwind willreturn
declare void @leaf() #0

; Function Attrs: mustprogress nofree nosync nounwind willreturn       <-- all WRONG
define void @caller() #1 {
  call void @leaf() [ "side_effects"() ]
  ret void
}
```

Expected: `@caller` should keep none of `nofree`, `nosync`, `nounwind`, `willreturn`,
or the derived `mustprogress`. The opbundle `"side_effects"()` is an
unknown/arbitrary bundle and per LangRef can free memory, synchronize, throw, and
fail to return.

The SCC variant also reproduces (each call inside the SCC carries an unknown bundle):

```llvm
define void @f() {
  call void @g() [ "side_effects"() ]
  ret void
}
define void @g() {
  call void @f() [ "side_effects"() ]
  ret void
}
```

`opt -passes=function-attrs -S` infers `nofree nosync nounwind` on both,
because:
- `InstrBreaksNoFree` returns false for `call @g`/`call @f` solely from SCC membership.
- `InstrBreaksNoSync` same; `maySynchronize` returns true (no NoSync on callee) but
  the SCC short-circuit fires.
- `InstrBreaksNonThrowing` same; `mayThrow` returns true but SCC short-circuit fires.

(`willreturn` and `mustprogress` are not inferred here only because the cycle
forms a backedge that `FindFunctionBackedges` in `functionWillReturn` detects.
Remove the recursion and they get inferred too — see the non-SCC case above.)

Cross-check: the `attributor` pass (also driven through `AANoSync`/`AANoFree`)
optimizes the same module to `memory(none)` callers and is not affected by this
particular oversight in the *legacy* per-instruction predicate, because Attributor's
update paths use `getMemoryEffects()` / `getOperandBundle`-aware queries throughout.

## Miscompile angle

A wrongly inferred `nofree` lets MemoryBuiltins / DSE / GVN / Inliner treat
`@caller` as a no-free callsite, so a heap pointer reachable through the bundle
can be considered dead and have its `malloc`/`store` eliminated. `nosync` lets
the LICM/loop-invariant-code-motion family hoist across the call. `nounwind`
disables EH unwind cleanup; `willreturn`/`mustprogress` enables removing the
call entirely under `mustprogress` semantics. The simplest end-to-end is

```llvm
declare void @leaf() nofree nosync nounwind willreturn
declare void @use(ptr)

define void @caller(ptr %p) {
  call void @leaf() [ "side_effects"() ]    ; opbundle could call free(%p)
  call void @use(ptr %p)                    ; depends on %p alive
  ret void
}
```
where `@caller` getting `nofree` allows propagating `nofree` to `@caller`'s
callsite and a subsequent pass concluding `%p` is still live, even though the
deopt-continuation-style bundle freed it. (To turn this into observable
miscompile in tree, the bundle's runtime-defined semantics must include
`free`, but the *attribute-level* unsoundness is independent of the bundle's
runtime behaviour — the pass is reasoning past a documented hole.)

## Fix sketch

In each `InstrBreaks*` helper, bail (return `true`) when the call has any
non-trivial operand bundle whose effects are not subsumed by the attribute
under test. Note `OB_deopt` is *not* safe for these predicates (it transitions
to an arbitrary continuation that may free/sync/throw/diverge), so the safe
whitelist is narrower than `hasClobberingOperandBundles` — match
`hasReadingOperandBundles` (Instructions.cpp:612-621), which already keeps
`deopt` in the "potentially observed" set:

```cpp
if (auto *CB = dyn_cast<CallBase>(&I))
  if (CB->hasOperandBundles() &&
      CB->hasOperandBundlesOtherThan({LLVMContext::OB_ptrauth,
                                      LLVMContext::OB_kcfi,
                                      LLVMContext::OB_convergencectrl,
                                      LLVMContext::OB_deactivation_symbol}))
    return true;  // bundle may free/sync/throw/diverge; can't infer
```
applied before the SCC short-circuit in `InstrBreaksNoFree`, `InstrBreaksNoSync`,
`InstrBreaksNonThrowing`, and (analogously) in `functionWillReturn` /
`Instruction::willReturn`.
