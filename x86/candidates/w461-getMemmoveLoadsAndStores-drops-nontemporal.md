# w461: SelectionDAG::getMemmoveLoadsAndStores drops !nontemporal

## Summary
Same bug class as w460, but for `llvm.memmove` instead of `llvm.memcpy`.
A memmove decorated with `!nontemporal` is expanded in-line into a
load-all-then-store-all sequence (the memmove idiom that lets the source
and destination overlap), and every one of the generated stores has its
MMO flags computed from `isVol` only — `MONonTemporal` is silently
discarded.

## Root cause (cite)
`llvm/lib/CodeGen/SelectionDAG/SelectionDAG.cpp`
- Line 9459-9465 (declaration of `getMemmoveLoadsAndStores`): takes
  `AAInfo` and uses it for stripped TBAA, but no parameter for
  `MachineMemOperand::Flags` from the source intrinsic.
- Line 9520-9521 (inside `getMemmoveLoadsAndStores`):
  ```cpp
  MachineMemOperand::Flags MMOFlags =
      isVol ? MachineMemOperand::MOVolatile : MachineMemOperand::MONone;
  ```
- Line 9605-9609: every destination store is built with this `MMOFlags`,
  which never includes `MONonTemporal`.

`llvm/lib/CodeGen/SelectionDAG/SelectionDAGBuilder.cpp`
- Line 6750-6754 (`Intrinsic::memmove`): passes `&I` to
  `DAG.getMemmove` but no per-instruction `MMOFlags`. The handler for
  `Intrinsic::memmove` does not test `MD_nontemporal` even though the
  generic `LoadInst`/`StoreInst` builders do
  (`SelectionDAGBuilder.cpp:4961-4962` and `5119-5120`).

## .ll
```llvm
target triple = "x86_64-unknown-linux-gnu"

declare void @llvm.memmove.p0.p0.i64(ptr nocapture writeonly,
                                     ptr nocapture readonly,
                                     i64, i1) nounwind

define void @memmove_nontemporal(ptr %dst, ptr %src) {
entry:
  call void @llvm.memmove.p0.p0.i64(ptr align 16 %dst, ptr align 16 %src,
                                    i64 32, i1 false), !nontemporal !0
  ret void
}

!0 = !{i32 1}
```

## MIR (`llc -O2 -mtriple=x86_64-unknown-linux-gnu -stop-after=finalize-isel`)
Actual:
```
body:             |
  bb.0.entry:
    liveins: $rdi, $rsi
    %1:gr64 = COPY $rsi
    %0:gr64 = COPY $rdi
    %2:vr128 = MOVAPSrm %1, 1, $noreg, 0, $noreg :: (load (s128) from %ir.src)
    %3:vr128 = MOVAPSrm %1, 1, $noreg, 16, $noreg :: (load (s128) from %ir.src + 16)
    MOVAPSmr %0, 1, $noreg, 16, $noreg, killed %3 :: (store (s128) into %ir.dst + 16)
    MOVAPSmr %0, 1, $noreg, 0, $noreg, killed %2 :: (store (s128) into %ir.dst)
    RET 0
```
Note: identical to the buggy w460 memcpy MIR. The MMO is `(store (s128) ...)`
not `(non-temporal store (s128) ...)`, and the opcode is `MOVAPSmr` not
`MOVNTPSmr`. The X86 selector already knows how to pick NT stores when
the MMO has the bit (see baseline in w460); the bit just never gets
set on this path.

## Why it matters
Same as w460: NT-streaming intent on memmove is dropped, costing the
cache-bypassing behaviour without any IR-level diagnostic.

## Fix sketch
Symmetric with w460 — pass `MachineMemOperand::Flags` through
`getMemmove` → `getMemmoveLoadsAndStores`, OR it into the local
`MMOFlags` at SelectionDAG.cpp:9520, set the flag in
`SelectionDAGBuilder::visitIntrinsicCall` for `Intrinsic::memmove` when
the instruction has `!nontemporal`.
