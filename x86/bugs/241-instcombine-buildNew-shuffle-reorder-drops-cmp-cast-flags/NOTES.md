# 241 — InstCombine `buildNew` (evaluateInDifferentElementOrder) drops ICmp `samesign`, FCmp FMF, and cast (`nneg`/`nuw`) flags

Component: `llvm/lib/Transforms/InstCombine/InstCombineVectorOps.cpp` lines ~2001-2026

The binary-op rebuild path correctly copies overflow/exact/FMF, but `Builder.CreateICmp`, `Builder.CreateFCmp`, and `Builder.CreateCast` leave the returned instruction with default flags. When a shuffle reorders an `icmp samesign`, an `fcmp nnan ninf`, a `zext nneg`, or a `trunc nuw` over the same operands, all those flags vanish.

## Reproducer

`opt -passes=instcombine -S repro.ll`

Input: `icmp samesign slt <2 x i32> %a, %b` followed by shufflevector. Output: `icmp slt <2 x i32> ...` — `samesign` lost.

## Severity

Default x86 -O2. Missed optimization — loss of poison-generating flags information.

## Fix

After each `Builder.CreateICmp/CreateFCmp/CreateCast`, call `NewI->copyIRFlags(I)`.
