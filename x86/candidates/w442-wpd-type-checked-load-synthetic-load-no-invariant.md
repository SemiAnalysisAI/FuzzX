# WPD `scanTypeCheckedLoadUsers` synthetic vtable load drops `!invariant.load`

## File and root cause

`llvm/lib/Transforms/IPO/WholeProgramDevirt.cpp` —
`DevirtModule::scanTypeCheckedLoadUsers` (line 2176, load at line 2215).

```cpp
Value *LoadedValue = nullptr;
if (TypeCheckedLoadFunc->getIntrinsicID() ==
    Intrinsic::type_checked_load_relative) {
  Function *LoadRelFunc = Intrinsic::getOrInsertDeclaration(
      &M, Intrinsic::load_relative, {Int32Ty});
  LoadedValue = LoadB.CreateCall(LoadRelFunc, {Ptr, Offset});
} else {
  Value *GEP = LoadB.CreatePtrAdd(Ptr, Offset);
  LoadedValue = LoadB.CreateLoad(Int8PtrTy, GEP);                  // <-- 2215
}

for (Instruction *LoadedPtr : LoadedPtrs) {
  LoadedPtr->replaceAllUsesWith(LoadedValue);
  LoadedPtr->eraseFromParent();
}
```

`Ptr` is the first argument of `llvm.type.checked.load`, i.e. the vtable
pointer. The intrinsic's documented semantics
(`LangRef.rst`/`Intrinsics.td`) say it performs an *aligned, invariant* load
of a function pointer from a vtable at a constant offset, with the type test
folded in. WPD lowers it to an explicit GEP + `load ptr, ptr %gep` — and
drops the implicit invariance that was part of the intrinsic's contract.

The original IR didn't have an explicit `LoadInst` for WPD to copy
`!invariant.load` from (the load is "inside" the intrinsic call), so this
isn't a metadata-dropping bug strictly speaking — it's a missing one-time
attachment when the intrinsic is lowered to plain LLVM IR.

## Reproducer

`x86/candidates/w442-wpd-type-checked-load-no-invariant.ll`:

```llvm
target datalayout = "e-p:64:64"

@vt1 = constant [3 x ptr] [ ptr @vf1, ptr @vf2, ptr @vf3 ], !type !0

define void @vf1(ptr %this) { ret void }
define void @vf2(ptr %this) { ret void }
define void @vf3(ptr %this) { ret void }

define void @test(ptr %obj) {
  %vtable = load ptr, ptr %obj
  %1 = call {ptr, i1} @llvm.type.checked.load(ptr %vtable, i32 0, metadata !"typeid")
  %fp = extractvalue {ptr, i1} %1, 0
  %tt = extractvalue {ptr, i1} %1, 1
  call void @llvm.assume(i1 %tt)
  call void %fp(ptr %obj)
  ret void
}

declare {ptr, i1} @llvm.type.checked.load(ptr, i32, metadata)
declare void @llvm.assume(i1)

!0 = !{i32 0, !"typeid"}
```

### `opt -S -passes=wholeprogramdevirt -whole-program-visibility` diff

After:
```llvm
define void @test(ptr %obj) {
  %vtable = load ptr, ptr %obj, align 8
  %1 = getelementptr i8, ptr %vtable, i32 0
  %2 = load ptr, ptr %1, align 8              ; <-- no !invariant.load
  call void @llvm.assume(i1 true)
  call void @vf1(ptr %obj)
  ret void
}
```

Note that single-impl devirt also runs and replaces the indirect `call %fp`
with `call @vf1`, so the load is dead. But in cases where single-impl doesn't
fire (e.g. multiple impls, no whole-program-visibility), the lowered load
survives all the way through, never marked invariant, and downstream LICM /
GVN cannot hoist or commonize it.

## Why this matters

* `llvm.type.checked.load` is the entry point for `clang`-emitted typed CFI
  virtual-call code on platforms where the intrinsic is supported. Every
  virtual call goes through it.
* Without `!invariant.load`, the lowered load can't be hoisted out of a loop
  even when the vtable pointer is loop-invariant.
* `Intrinsic::load_relative` (the relative-vtable variant on line 2212) at
  least has `memory(read)` and gets some of the way; the regular
  `LoadInst` path on line 2215 is the one missing the hint.

## Fix sketch

```cpp
LoadInst *LoadedLI = LoadB.CreateLoad(Int8PtrTy, GEP);
LoadedLI->setMetadata(LLVMContext::MD_invariant_load,
                      MDNode::get(M.getContext(), {}));
LoadedValue = LoadedLI;
```
