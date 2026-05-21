# 227 — `AtomicExpandPass::expandAtomicLoadToCmpXchg` crash on vector atomic load — verifier rejects synthesized cmpxchg

Component: `llvm/lib/CodeGen/AtomicExpandPass.cpp` lines ~668-687

`expandAtomicLoadToCmpXchg` unconditionally uses `LI->getType()` for the cmpxchg operand types (lines 675-679). Sibling `createCmpXchgInstFun` (lines 737-765) correctly bitcasts non-integer types. The X86 lowering hook `shouldCastAtomicLoadInIR` only casts FP-scalar element types; integer-element vectors (`<2 x i64>`, `<4 x i32>`, `<8 x i16>`, `<16 x i8>`) bypass the cast and reach `expandAtomicLoadToCmpXchg`, synthesizing an illegal `cmpxchg ptr, <vec-ty>, <vec-ty>` that the IR verifier rejects with "cmpxchg operand must have integer or pointer type".

## Reproducer

`llc -O2 -mtriple=x86_64-unknown-linux-gnu -mattr=+cx16,-avx repro.ll -o -` crashes with `LLVM ERROR: Broken function found, compilation aborted!`.

The symmetric `store atomic <2 x i64>` succeeds because it routes through `createCmpXchgInstFun` (which bitcasts). Only the load path is broken.

## Severity

Hard crash on a valid IR pattern. Default x86 -O2 with cx16 support.

## Fix

In `expandAtomicLoadToCmpXchg`, bitcast non-integer/non-pointer types to an integer of the same size before constructing the cmpxchg, mirroring `createCmpXchgInstFun`.
