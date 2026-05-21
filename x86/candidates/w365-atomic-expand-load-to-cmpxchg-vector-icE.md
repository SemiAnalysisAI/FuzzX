# w365: AtomicExpandPass expandAtomicLoadToCmpXchg crashes on vector atomic load (illegal cmpxchg + verifier abort)

## Root cause

`AtomicExpandImpl::expandAtomicLoadToCmpXchg`
(llvm/lib/CodeGen/AtomicExpandPass.cpp:668-687) lowers an `atomic load` whose
type the target said cannot be loaded natively into a `cmpxchg ptr, 0, 0` that
discards the success bit and keeps the loaded value. The function uses
`LI->getType()` directly as the type of the dummy compare/new value:

```
Value *Addr = LI->getPointerOperand();
Type *Ty = LI->getType();
Constant *DummyVal = Constant::getNullValue(Ty);

Value *Pair = Builder.CreateAtomicCmpXchg(
    Addr, DummyVal, DummyVal, LI->getAlign(), Order,
    AtomicCmpXchgInst::getStrongestFailureOrdering(Order));
```
(AtomicExpandPass.cpp:674-680)

If `Ty` is a vector type, this builds an `AtomicCmpXchgInst` with vector
operands, which the IR verifier rejects: "cmpxchg operand must have integer
or pointer type" (see `Verifier::visitAtomicCmpXchgInst`,
llvm/lib/IR/Verifier.cpp). The verifier call from `legacy::FunctionPassManager`
then triggers `report_fatal_error("Broken function found, compilation aborted!")`.

The sibling helper `createCmpXchgInstFun` (AtomicExpandPass.cpp:737-765)
already has the correct pattern:
```
bool NeedBitcast = OrigTy->isFloatingPointTy() || OrigTy->isVectorTy();
if (NeedBitcast) {
  IntegerType *IntTy = Builder.getIntNTy(OrigTy->getPrimitiveSizeInBits());
  NewVal = Builder.CreateBitCast(NewVal, IntTy);
  Loaded = Builder.CreateBitCast(Loaded, IntTy);
}
```
Without an equivalent bitcast in `expandAtomicLoadToCmpXchg`, the cmpxchg is
constructed with the vector type as-is.

## X86 trigger chain

1. `X86TargetLowering::shouldCastAtomicLoadInIR` (X86ISelLowering.cpp:32999-33004)
   only returns `CastToInteger` if the *scalar* element type is floating
   point. For `<N x iK>` it returns `None` -> no precast happens.
2. `X86TargetLowering::shouldExpandAtomicLoadInIR` (X86ISelLowering.cpp:32490-32510)
   returns `CmpXChg` for any 128-bit load on x86_64 unless AVX-128 atomic load
   is available. With `cx16` and no AVX, the path is taken.
3. Pre-cast happened for the FP scalar element vector
   (`<4 x float>` -> bitcast to `i128`) so those work. Integer-element
   vectors like `<2 x i64>`, `<4 x i32>`, `<8 x i16>`, `<16 x i8>` reach
   `expandAtomicLoadToCmpXchg` unchanged.
4. `expandAtomicLoadToCmpXchg` builds the illegal cmpxchg. Verifier aborts.

## Reproducer

A kernel-style function (e.g. `no-implicit-float` with `+cx16`) is enough to
trigger this from the default x86_64 backend without any extra `-mattr`:

```
target triple = "x86_64-unknown-linux-gnu"

define <2 x i64> @load_v2i64(ptr %p) #0 {
  %v = load atomic <2 x i64>, ptr %p seq_cst, align 16
  ret <2 x i64> %v
}

define <4 x i32> @load_v4i32(ptr %p) #0 {
  %v = load atomic <4 x i32>, ptr %p seq_cst, align 16
  ret <4 x i32> %v
}

define <8 x i16> @load_v8i16(ptr %p) #0 {
  %v = load atomic <8 x i16>, ptr %p seq_cst, align 16
  ret <8 x i16> %v
}

define <16 x i8> @load_v16i8(ptr %p) #0 {
  %v = load atomic <16 x i8>, ptr %p seq_cst, align 16
  ret <16 x i8> %v
}

attributes #0 = { "no-implicit-float" "target-features"="+cx16" }
```

Invocation:
```
llc -O2 -mtriple=x86_64-unknown-linux-gnu repro.ll
```

Equivalent without function attributes (but with command-line flags):
```
llc -O2 -mtriple=x86_64-unknown-linux-gnu -mattr=+cx16,-avx repro.ll
```

IR after atomic-expand for `<2 x i64>` (broken):
```
define <2 x i64> @load_v2i64(ptr %p) #0 {
  %1 = cmpxchg ptr %p, <2 x i64> zeroinitializer, <2 x i64> zeroinitializer seq_cst seq_cst, align 16
  %loaded = extractvalue { <2 x i64>, i1 } %1, 0
  ret <2 x i64> %loaded
}
```

Then:
```
cmpxchg operand must have integer or pointer type
 <2 x i64>  %1 = cmpxchg ptr %p, ...
in function load_v2i64
LLVM ERROR: Broken function found, compilation aborted!
```

The crash is deterministic on any vector type whose scalar element is an
integer (i.e. not caught by `shouldCastAtomicLoadInIR`'s FP-only check) and
whose total size makes `needsCmpXchgNb` return true (128-bit on x86_64+cx16).

## Why the sibling store path works

The mirror code for atomic *store* goes through
`expandAtomicStoreToXChg` -> `tryExpandAtomicRMW(Xchg)` ->
`expandAtomicRMWToCmpXchg` -> `insertRMWCmpXchgLoop` ->
`createCmpXchgInstFun`. The `createCmpXchgInstFun` callback at line 737-765
bitcasts vector/FP types to an equivalent iN before issuing the cmpxchg, so
the same workload via store does NOT crash:

```
define void @store_v2i64(ptr %p, <2 x i64> %v) #0 {
  %1 = load <2 x i64>, ptr %p, align 16
  br label %atomicrmw.start
atomicrmw.start:
  %loaded = phi <2 x i64> [ %1, %0 ], [ %5, %atomicrmw.start ]
  %2 = bitcast <2 x i64> %v to i128                ; <-- here
  %3 = bitcast <2 x i64> %loaded to i128           ; <-- here
  %4 = cmpxchg ptr %p, i128 %3, i128 %2 seq_cst seq_cst, align 16
  ...
}
```

So load + cmpxchg expansion for vectors is the only crash path; the
asymmetry directly points at the missing bitcast in
`expandAtomicLoadToCmpXchg`.

## Fix

Add the bitcast-to-integer dance from `createCmpXchgInstFun` to
`expandAtomicLoadToCmpXchg`. After computing `Ty = LI->getType()`:

```
Type *CXTy = Ty;
bool NeedBitcast = Ty->isFloatingPointTy() || Ty->isVectorTy();
if (NeedBitcast)
  CXTy = Builder.getIntNTy(DL->getTypeStoreSizeInBits(Ty));
Constant *DummyVal = Constant::getNullValue(CXTy);
Value *Pair = Builder.CreateAtomicCmpXchg(
    Addr, DummyVal, DummyVal, LI->getAlign(), Order,
    AtomicCmpXchgInst::getStrongestFailureOrdering(Order),
    LI->getSyncScopeID());                       // also fixes #066
cast<AtomicCmpXchgInst>(Pair)->setVolatile(LI->isVolatile());
copyMetadataForAtomic(*cast<Instruction>(Pair), *LI);
Value *Loaded = Builder.CreateExtractValue(Pair, 0, "loaded");
if (NeedBitcast)
  Loaded = Builder.CreateBitCast(Loaded, Ty);
LI->replaceAllUsesWith(Loaded);
LI->eraseFromParent();
```

Alternative fix: extend `X86TargetLowering::shouldCastAtomicLoadInIR` to
also return `CastToInteger` for vectors (mirroring how FP scalars are
handled), which would route vector atomics through
`convertAtomicLoadToIntegerType` first. Either fix prevents the crash;
the in-source fix is more robust (applies to every target that hits
`expandAtomicLoadToCmpXchg` with a non-integer atomic load).

## Related bugs

- #066 (`w66-atomic-expand-load-to-cmpxchg-drops-volatile-syncscope.md`):
  same call site, separate defect (volatile/syncscope drop). The fix
  sketch above incorporates both.
- #108 (`w108-convertAtomicLoadToIntegerType-drops-tbaa-noalias.md`):
  sibling cast helper drops AA metadata; this entry covers a hard crash
  on an entirely separate code path.
- This is target-independent code in AtomicExpandPass.cpp; same broken IR
  would be produced on any target whose `shouldExpandAtomicLoadInIR`
  returns `CmpXChg` for a vector-typed load that bypasses the FP cast hook.
