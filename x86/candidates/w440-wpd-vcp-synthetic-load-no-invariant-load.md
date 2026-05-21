# WPD virtual-const-prop synthetic load drops `!invariant.load`

## File and root cause

`llvm/lib/Transforms/IPO/WholeProgramDevirt.cpp` —
`DevirtModule::applyVirtualConstProp` (line 1873).

```cpp
void DevirtModule::applyVirtualConstProp(CallSiteInfo &CSInfo, StringRef FnName,
                                         Constant *Byte, Constant *Bit) {
  for (auto Call : CSInfo.CallSites) {
    if (!OptimizedCalls.insert(&Call.CB).second)
      continue;
    auto *RetType = cast<IntegerType>(Call.CB.getType());
    IRBuilder<> B(&Call.CB);
    Value *Addr = B.CreatePtrAdd(Call.VTable, Byte);
    if (RetType->getBitWidth() == 1) {
      Value *Bits = B.CreateLoad(Int8Ty, Addr);             // <-- line 1882
      Value *BitsAndBit = B.CreateAnd(Bits, Bit);
      auto IsBitSet = B.CreateICmpNE(BitsAndBit, ConstantInt::get(Int8Ty, 0));
      NumVirtConstProp1Bit++;
      Call.replaceAndErase("virtual-const-prop-1-bit", FnName, RemarksEnabled,
                           OREGetter, IsBitSet);
    } else {
      Value *Val = B.CreateLoad(RetType, Addr);             // <-- line 1889
      NumVirtConstProp++;
      Call.replaceAndErase("virtual-const-prop", FnName, RemarksEnabled,
                           OREGetter, Val);
    }
  }
  CSInfo.markDevirt();
}
```

Both `CreateLoad` calls produce a fresh `LoadInst` with **no metadata**. The
address being loaded is `vtable + Byte` — and the byte/bits at that offset
live inside the anonymous struct global produced by
`DevirtModule::rebuildGlobal` (line 2059–2068):

```cpp
auto *NewInit = ConstantStruct::getAnon(
    {ConstantDataArray::get(M.getContext(), B.Before.Bytes),
     B.GV->getInitializer(),
     ConstantDataArray::get(M.getContext(), B.After.Bytes)});
auto *NewGV =
    new GlobalVariable(M, NewInit->getType(), B.GV->isConstant(),
                       GlobalVariable::PrivateLinkage, NewInit, "", B.GV);
```

`B.GV->isConstant()` is propagated to the new global — for vtables this is
`true`. The data WPD reads with the synthetic load therefore lives in a
`constant` global, i.e. is genuinely invariant. The vtable load that produced
`Call.VTable` in the original IR (the Itanium-ABI vtable pointer) is also
typically `!invariant.load` (Clang attaches it on vtable loads). The synthetic
load reading from the same constant object should also be marked
`!invariant.load`, but isn't.

## Reproducer

`x86/candidates/w440-wpd-vcp-load-no-invariant.ll`:

```llvm
target datalayout = "e-p:64:64"

@vt1 = constant [3 x ptr] [ ptr @vf0i16, ptr @vfA, ptr @vfB ], !type !0
@vt2 = constant [3 x ptr] [ ptr @vf2i16, ptr @vfA, ptr @vfB ], !type !0
@vt3 = constant [3 x ptr] [ ptr @vf5i16, ptr @vfA, ptr @vfB ], !type !0

define i16 @vf0i16(ptr %this) readnone { ret i16 0 }
define i16 @vf2i16(ptr %this) readnone { ret i16 2 }
define i16 @vf5i16(ptr %this) readnone { ret i16 5 }
define void @vfA(ptr %this) { ret void }
define void @vfB(ptr %this) { ret void }

define i16 @call_vcp(ptr %obj) {
  %vtable = load ptr, ptr %obj, !invariant.load !100
  %p = call i1 @llvm.type.test(ptr %vtable, metadata !"typeid")
  call void @llvm.assume(i1 %p)
  %fptr = load ptr, ptr %vtable, !invariant.load !100
  %result = call i16 %fptr(ptr %obj)
  ret i16 %result
}

declare i1 @llvm.type.test(ptr, metadata)
declare void @llvm.assume(i1)

!0 = !{i32 0, !"typeid"}
!100 = !{}
```

### `opt -S -passes=wholeprogramdevirt -whole-program-visibility` diff

Before (input): user code calls a virtual function returning `i16`. Two `load`
instructions both carry `!invariant.load`.

After:

```llvm
define i16 @call_vcp(ptr %obj) {
  %vtable = load ptr, ptr %obj, align 8, !invariant.load !1
  %p = call i1 @llvm.type.test(ptr %vtable, metadata !"typeid")
  call void @llvm.assume(i1 %p)
  %fptr = load ptr, ptr %vtable, align 8, !invariant.load !1
  %1 = getelementptr i8, ptr %vtable, i32 -2
  %2 = load i16, ptr %1, align 2       ; <-- NO !invariant.load
  ret i16 %2
}
```

The synthesized `load i16, ptr %1` is the WPD optimization payload. It reads
from `vtable - 2`, which is part of the constant data that
`rebuildGlobal` placed before the vtable initializer:

```llvm
@1 = private constant { [8 x i8], [3 x ptr], [0 x i8] }
       { [8 x i8] c"\00\00\00\00\00\00\02\00", [3 x ptr] [...], [...] }
```

The same applies for the i1 (1-bit-prop) code path — see
`x86/candidates/w440-wpd-vcp-1bit-load-no-invariant.ll`, which produces:

```llvm
  %1 = getelementptr i8, ptr %vtable, i32 -1
  %2 = load i8, ptr %1, align 1       ; <-- NO !invariant.load
  %3 = and i8 %2, 1
```

## Why this matters

* Downstream passes (LICM, GVN, EarlyCSE) lose the explicit invariance signal.
  AA must re-prove it from the underlying constant global, which it cannot
  always do across a function-pointer GEP from `%vtable`.
* The original vtable load already carries `!invariant.load`. The new load
  reads from the same constant object via a constant-offset GEP, so the
  metadata applies to it just as soundly.
* `!invariant.load` would also let a subsequent loop-invariant hoist move
  the propagated value out of the loop — currently only the unhoistable form
  survives WPD, undoing the point of vcp in hot loops.

## Fix sketch

Attach `!invariant.load` to both synthetic loads:

```cpp
LoadInst *Bits = B.CreateLoad(Int8Ty, Addr);
Bits->setMetadata(LLVMContext::MD_invariant_load,
                  MDNode::get(M.getContext(), {}));
```

and likewise for the wide load on line 1889.

The same applies to `lowerTypeTestCalls` for `type_checked_load` lowering
(`WholeProgramDevirt.cpp:2215`), where the load reads a function pointer
from the vtable — a separate but identical hole.
