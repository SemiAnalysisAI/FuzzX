# w294 -- SROA `unfoldGEPSelect` / `unfoldGEPPhi` drop GEP metadata (`!annotation`, `!nosanitize`) on the rewritten GEPs

## Component
`llvm/lib/Transforms/Scalar/SROA.cpp`:
- `AggLoadStoreRewriter::unfoldGEPSelect`, lines 4229-4319
- `AggLoadStoreRewriter::unfoldGEPPhi`, lines 4325-4415

Both routines rewrite `gep (select c, p1, p2), idx` -> `select c, gep(p1, idx),
gep(p2, idx)` and `gep (phi p1, p2), idx` -> `phi (gep(p1, idx), gep(p2, idx))`
by calling `IRB.CreateGEP` with the original `GEPI.getNoWrapFlags()` (lines
4294-4299 for select; line 4396 for phi):

```cpp
GEPNoWrapFlags NW = GEPI.getNoWrapFlags();
Type *Ty = GEPI.getSourceElementType();
Value *NTrue = IRB.CreateGEP(Ty, TrueOps[0], ArrayRef(TrueOps).drop_front(),
                             True->getName() + ".sroa.gep", NW);
Value *NFalse = IRB.CreateGEP(Ty, FalseOps[0], ArrayRef(FalseOps).drop_front(),
                              False->getName() + ".sroa.gep", NW);
```

After this, the original `GEPI` is erased (line 4309 / 4404). The only
GEP-instruction state preserved is the `GEPNoWrapFlags`. The source GEP's
**instruction metadata is silently dropped** on the new GEPs, including:

- `!annotation` -- a generic structured-annotation marker the front-end may
  use to track e.g. specialized profiling or AutoFDO hints
- `!nosanitize` -- a marker that *opts out* this specific pointer arithmetic
  from UBSan/ASan/MSan instrumentation
- `!dbg` debug location is *not* explicitly set from `GEPI.getDebugLoc()`; it
  is whatever `IRB` happened to carry. (`unfoldGEPSelect` sets the insert
  point to `&GEPI` at line 4290, but `SetInsertPoint(Instruction*)` does not
  update `CurDbgLocation` -- so the new GEPs inherit a stale debug location
  from some earlier op.)

## Reproducer (unfoldGEPSelect)
```ll
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

%S = type { i32, i32 }

define i32 @test(i1 %c, ptr %ext) {
entry:
  %a = alloca %S, align 4
  store i32 100, ptr %a, align 4
  %sel = select i1 %c, ptr %a, ptr %ext
  %g = getelementptr inbounds %S, ptr %sel, i32 0, i32 1, !annotation !0, !nosanitize !1
  %v = load i32, ptr %g, align 4
  ret i32 %v
}

!0 = !{!"important"}
!1 = !{}
```

`opt -passes=sroa -S`:
```ll
define i32 @test(i1 %c, ptr %ext) {
entry:
  %ext.sroa.gep = getelementptr inbounds %S, ptr %ext, i32 0, i32 1  ; <-- !annotation, !nosanitize gone
  br i1 %c, label %entry.cont, label %entry.else

entry.else:                                       ; preds = %entry
  %v.else.val = load i32, ptr %ext.sroa.gep, align 4
  br label %entry.cont

entry.cont:                                       ; preds = %entry, %entry.else
  %v = phi i32 [ undef, %entry ], [ %v.else.val, %entry.else ]
  ret i32 %v
}
```

The new GEP `%ext.sroa.gep` carries no `!annotation` and no `!nosanitize`.

## Reproducer (unfoldGEPPhi)
```ll
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

%S = type { i32, i32 }

define i32 @test(i1 %c, ptr %ext) {
entry:
  %a = alloca %S, align 4
  store i32 100, ptr %a, align 4
  br i1 %c, label %L, label %R
L:
  br label %J
R:
  br label %J
J:
  %phi = phi ptr [ %a, %L ], [ %ext, %R ]
  %g = getelementptr inbounds %S, ptr %phi, i32 0, i32 1, !annotation !0, !nosanitize !1
  %v = load i32, ptr %g, align 4
  ret i32 %v
}

!0 = !{!"important"}
!1 = !{}
```

`opt -passes=sroa -S` produces the same shape: `%phi.sroa.gep1 = getelementptr
inbounds %S, ptr %ext, i32 0, i32 1` with no `!annotation` / `!nosanitize`.

## Impact
- **`!nosanitize` is a sanitizer-correctness directive.** When a front-end
  (Clang) emits `!nosanitize` on a GEP because the source-language semantics
  permit otherwise-instrumented arithmetic (e.g. one-past-the-end for sentinel
  iteration, certain `std::array` end iterators), SROA silently strips the
  marker. The next pass (UBSan / ASan instrumentation, or any pass that uses
  `Instruction::hasMetadata(MD_nosanitize)` as a gate) can then re-instrument
  the GEP. This causes spurious sanitizer reports in optimized builds and is
  a *behavior change* even at default -O2 because passes like
  `ConstantFolding`, `InstCombine`, and `LoopUnroll` query `!nosanitize` to
  refuse to fold/transform.
- `!annotation` loss breaks AutoFDO / sample-PGO / structured-debug pipelines
  that rely on the marker to correlate IR with source intent.
- Stale `!dbg` on the new GEPs misleads source-level debuggers/profilers about
  the location of the speculatively computed pointer.

## Fix sketch
After creating each new GEP, call `cast<Instruction>(NewGEP)->copyMetadata(
GEPI, {LLVMContext::MD_annotation, LLVMContext::MD_nosanitize,
LLVMContext::MD_pcsections, LLVMContext::MD_DIAssignID})` or use the
all-metadata `copyMetadata(GEPI)` overload. Also call
`IRB.SetCurrentDebugLocation(GEPI.getDebugLoc())` before creating the new
GEPs in both `unfoldGEPSelect` and `unfoldGEPPhi`.

## Notes
- Default x86 -O2 only. Confirmed on LLVM 23.0.0git (FuzzX `opt` build).
- Distinct from w290-w293 which are load/store metadata drops; this is a
  GEP-instruction metadata drop in a different SROA routine.
- Distinct from w97 which is memcpy-split TBAA substitution.
