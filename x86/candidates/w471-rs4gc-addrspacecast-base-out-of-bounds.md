## RS4GC produces gc.relocate with out-of-bounds base index for `addrspacecast` to GC address space

`llvm/lib/Transforms/Scalar/RewriteStatepointsForGC.cpp:496-510` (`findBaseDefiningValue` CastInst arm)

```cpp
if (CastInst *CI = dyn_cast<CastInst>(I)) {
  Value *Def = CI->stripPointerCasts();
  // If stripping pointer casts changes the address space there is an
  // addrspacecast in between.
  assert(cast<PointerType>(Def->getType())->getAddressSpace() ==
             cast<PointerType>(CI->getType())->getAddressSpace() &&
         "unsupported addrspacecast");
  // ...
  auto *BDV = findBaseDefiningValue(Def, Cache, KnownBases);
  Cache[CI] = BDV;
  return BDV;
}
```

`AddrSpaceCastInst` is a subclass of `CastInst`, so this branch fires for
`addrspacecast ptr %p to ptr addrspace(1)`. `stripPointerCasts()` walks
through the addrspacecast, returning the addrspace(0) value `%p`. The
addrspace(0) result is then cached as the BDV for the addrspace(1) cast.

The "support assertion" at line 500-502 is the only defense. In NDEBUG
builds (this fuzzer build is `Optimized`) the assertion is elided and the
pass silently records an addrspace(0) base for an addrspace(1) derived
pointer. The base is not a GC pointer, so it is NOT added to the
gc-live bundle of the statepoint. But the gc.relocate intrinsic is still
emitted with a base index that came from `FindIndex(LiveVariables, base)`
at line 1509-1515:

```cpp
auto FindIndex = [](ArrayRef<Value *> LiveVec, Value *Val) {
  auto ValIt = llvm::find(LiveVec, Val);
  assert(ValIt != LiveVec.end() && "Val not found in LiveVec!");
  size_t Index = std::distance(LiveVec.begin(), ValIt);
  ...
};
```

When `Val` is absent the assert is elided and `std::distance` returns
`LiveVec.size()`, an out-of-bounds index. The emitted gc.relocate therefore
references "one past the end" of the gc-live bundle. The IR verifier rejects
the result.

### Candidate IR

```
target triple = "x86_64-unknown-linux-gnu"

declare void @bar()
declare token @llvm.experimental.gc.statepoint.p0(i64, i32, ptr, i32, i32, ...)

define ptr addrspace(1) @test(ptr %p) gc "statepoint-example" {
  %p1 = addrspacecast ptr %p to ptr addrspace(1)
  call void @bar()
  ret ptr addrspace(1) %p1
}
```

### Observed (wrong) output

`opt -passes=rewrite-statepoints-for-gc -disable-verify -S`:

```
define ptr addrspace(1) @test(ptr %p) gc "statepoint-example" {
  %p1 = addrspacecast ptr %p to ptr addrspace(1)
  %statepoint_token = call token (...) @llvm.experimental.gc.statepoint.p0(
       i64 2882400000, i32 0, ptr elementtype(void ()) @bar, i32 0, i32 0, i32 0, i32 0)
       [ "gc-live"(ptr addrspace(1) %p1) ]
  %p1.relocated = call coldcc ptr addrspace(1)
       @llvm.experimental.gc.relocate.p1(token %statepoint_token, i32 1, i32 0)
                                                                  ;       ^^^^^
                                                                  ; OUT OF BOUNDS: gc-live has 1 entry (index 0 only)
       ; (@llvm.experimental.gc.statepoint.p0, %p1)
       ; The decoder interprets index 1 as the statepoint function itself.
  ret ptr addrspace(1) %p1.relocated
}
```

Without `-disable-verify`, the module verifier aborts:

```
gc.relocate: statepoint base index out of bounds
  %p1.relocated = call coldcc ptr addrspace(1)
      @llvm.experimental.gc.relocate.p1(token %statepoint_token, i32 1, i32 0)
LLVM ERROR: Broken module found, compilation aborted!
```

### Expected wrong outcome

Two failure modes:

1. **Front-end / IR-producer crash**: any tool that pipes through the IR
   verifier after RS4GC (which the LegacyPM and the NPM both do by default)
   crashes with a "Broken module" error.
2. **Silent miscompile** when run with `-disable-verify`: the GC will read
   the out-of-bounds slot during stack-map decoding, which is either garbage
   memory or, with the specific layout above, the statepoint's own function
   pointer being treated as a heap base. The relocated value handed back to
   the program is then derived from a non-GC base, breaking any subsequent
   collector-driven object movement.

The root cause is the un-handled `addrspacecast` whose source and destination
have different address spaces. The CastInst arm should special-case
`AddrSpaceCastInst` and either (a) treat the cast itself as a BDV (its
own base, mirroring the `inttoptr` arm at line 490-494) when the source is
not a GC pointer, or (b) refuse to look through an addrspacecast that
crosses the GC address-space boundary.

### Cross-reference

The comment at line 498-502 explicitly acknowledges the case (
"If stripping pointer casts changes the address space there is an addrspacecast
in between") and chooses to assert rather than handle it. The assertion's
`"unsupported addrspacecast"` message would only fire in a debug build.

The IntToPtr arm at line 490-494 already has the correct pattern:

```cpp
if (isa<IntToPtrInst>(I)) {
  Cache[I] = I;
  setKnownBase(I, /* IsKnownBase */true, KnownBases);
  return I;
}
```

The analogous treatment for an `addrspacecast` whose source is in a
non-GC address space is missing.

### Reproducer

`/tmp/rs4gc_test/t_ascast_simpler.ll`
