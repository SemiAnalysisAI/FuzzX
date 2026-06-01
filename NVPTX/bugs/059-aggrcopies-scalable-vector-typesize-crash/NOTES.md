# 059 — NVPTXLowerAggrCopies crashes (fatal internal error) on a scalable-vector aggregate load/store: scalable TypeSize implicitly converted to fixed unsigned in the size comparison

- **Kind:** crash (fatal/UB)
- **Reachable via:** default llc -O0
- **Component:** NVPTXLowerAggrCopies.cpp 74 (also 107)  (round-8 area `X04-pass-crash2`)

## Summary

a scalable-vector `load;store` pair makes NVPTXLowerAggrCopies convert a scalable TypeSize to fixed → `reportFatalInternalError` (distinct site from #047)

## Mechanism / root cause

In the aggregate-load collection loop, line 74 does:

    if (DL.getTypeStoreSize(LI->getType()) < MaxAggrCopySize)
      continue;

`MaxAggrCopySize` is `static const unsigned = 128`. `DL.getTypeStoreSize(LI->getType())` returns a `TypeSize`. When `LI->getType()` is a scalable vector (e.g. <vscale x 4 x i32>), the store size is a *scalable* TypeSize. The comparison against `unsigned` forces the implicit `TypeSize::operator ScalarTy()` conversion, which for a scalable size executes `reportFatalInternalError("Cannot implicitly convert a scalable size to a fixed-width size in TypeSize::operator ScalarTy()")` and aborts the compiler. (Line 107, `unsigned NumLoads = DL.getTypeStoreSize(LI->getType());`, would crash identically on the same path.)

Why this is a bug and not a graceful diagnostic: the input IR is well-formed (a load/store of a sized scalable vector is legal LLVM IR; `opt -passes=verify` accepts it). The pass runs unconditionally in the NVPTX codegen pipeline (NVPTXTargetMachine.cpp). The abort is an internal `reportFatalInternalError`, not a target-capability 'cannot select'/'unsupported' diagnostic, so it counts as a compiler crash from valid input. The fix used elsewhere for fixed-only assumptions is `getFixedValue()`/`isScalable()` guarding (cf. how other size code checks scalability before extracting a scalar).

This is DISTINCT from README #047 (NVPTXLowerArgs::copyByValParam, `AllocaInst::getAllocationSize` -> CreateMemCpy length; a byval kernel param path) and from README #007 (the overlap miscompile in the same file). The crash site here is NVPTXLowerAggrCopies::runOnFunction directly (confirmed stack frame #9 = NVPTXLowerAggrCopies::runOnFunction), triggered by an ordinary scalable-vector load whose single use is a store -- no byval, no kernel, no overlap needed.

## Trigger

A plain device function (not necessarily a kernel) containing `%v = load <vscale x N x T>, ptr %src` whose single user is `store <vscale x N x T> %v, ptr %dst`. Reaches the pass at -O0 (where the LoadStoreVectorizer, which would otherwise crash first on the same IR at -O2, is not run). Any scalable element count/type works (size of the scalable vector is irrelevant -- the crash is on the scalable->scalar conversion in the size comparison itself, before the 128-byte threshold matters).

## Reproducer

```
target triple = "nvptx64-nvidia-cuda"

define void @copy_sv(ptr %dst, ptr %src) {
  %v = load <vscale x 4 x i32>, ptr %src
  store <vscale x 4 x i32> %v, ptr %dst
  ret void
}
```

Command: `llc -O0 -mtriple=nvptx64 -o - repro.ll`

## Verification

Reproduced with the built llc (crash/emitted-PTX matches; finder confidence 0.92). 
