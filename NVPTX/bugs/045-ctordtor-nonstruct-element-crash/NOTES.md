# 045 — NVPTXCtorDtorLowering: cast<ConstantStruct> crashes on a zeroinitializer/poison element of llvm.global_ctors/global_dtors

- **Kind:** crash (assert/UB)
- **Reachable via:** default llc
- **Component:** NVPTXCtorDtorLowering.cpp 179-182 (crash at line 180)  (round-7 area `C03-pass-crash`)

## Summary

`llvm.global_ctors`/`global_dtors` with a `zeroinitializer`/`poison` array element crashes `cast<ConstantStruct>` in NVPTXCtorDtorLowering

## Mechanism / root cause

createInitOrFiniGlobals() does `ConstantArray *GA = dyn_cast<ConstantArray>(GV->getInitializer());` then iterates: `for (Value *V : GA->operands()) { auto *CS = cast<ConstantStruct>(V); auto *F = cast<Constant>(CS->getOperand(1)); uint64_t Priority = cast<ConstantInt>(CS->getOperand(0))->getSExtValue(); ... }`. The code assumes every element of the global_ctors/global_dtors array is a ConstantStruct. But an array element that is `{i32,ptr,ptr} zeroinitializer` is a ConstantAggregateZero, and `... poison` is a PoisonValue — neither is a ConstantStruct. (The whole-array zeroinitializer case is handled because the outer dyn_cast<ConstantArray> fails, but a per-element zero in an otherwise-ConstantArray is not.) `cast<ConstantStruct>(V)` then fails: in an assertions build it hits `assert(isa<To>(Val) && "cast<Ty>() argument of incompatible type!")` (Casting.h:572); in a release build it is an unchecked static_cast -> UB. The LangRef permits global_ctors/global_dtors array elements to be any constant of the struct type, and the IR verifier accepts a zeroinitializer/poison element (verified: `opt -passes=verify` exits 0). So this is a compiler crash on valid, verifier-accepted input.

## Trigger

A module with @llvm.global_ctors (or @llvm.global_dtors) whose initializer is a ConstantArray with at least one real `{i32,ptr,ptr}` struct entry and at least one element that is `{i32,ptr,ptr} zeroinitializer` or `... poison`. Reached by default codegen pipeline (the pass runs unconditionally for nvptx); no special -mcpu needed (reproduced sm_60).

## Reproducer

```
target triple = "nvptx64-nvidia-cuda"
declare void @ctor()
@llvm.global_ctors = appending global [2 x { i32, ptr, ptr }] [
  { i32, ptr, ptr } { i32 65535, ptr @ctor, ptr null },
  { i32, ptr, ptr } zeroinitializer ]
```

Command: `llc -mtriple=nvptx64 -mcpu=sm_60 -o - repro.ll`

## Verification

Reproduced with the built llc (crash/emitted-PTX matches the claim; finder confidence 0.95).
