# w462: SelectionDAG::getMemsetStores drops !nontemporal

## Summary
Final sibling of w460/w461. An `llvm.memset` decorated with
`!nontemporal` and lowered inline emits a series of stores none of which
carry `MONonTemporal`, so the X86 back end never emits any of the NT
store variants (MOVNTPS / MOVNTDQ / MOVNTI). The IR-level NT intent is
silently dropped, costing the cache-bypass.

## Root cause (cite)
`llvm/lib/CodeGen/SelectionDAG/SelectionDAG.cpp`
- Line 9636-9640 (`getMemsetStores` signature): takes `AAInfo` but no
  `MachineMemOperand::Flags` from the source intrinsic.
- Line 9747-9752 (store builder inside the per-chunk loop):
  ```cpp
  SDValue Store = DAG.getStore(
      Chain, dl, Value,
      DAG.getObjectPtrOffset(dl, Dst, TypeSize::getFixed(DstOff)),
      DstPtrInfo.getWithOffset(DstOff), Alignment,
      isVol ? MachineMemOperand::MOVolatile : MachineMemOperand::MONone,
      NewAAInfo);
  ```
  The flag word is computed entirely from `isVol` — `MONonTemporal` is
  never threaded in.

`llvm/lib/CodeGen/SelectionDAG/SelectionDAGBuilder.cpp`
- Line 6731-6733 (`Intrinsic::memset`): hands `&I` to `DAG.getMemset`
  but never queries `I.hasMetadata(MD_nontemporal)`. Compare to the
  ordinary store path at line 5119-5120.

## .ll
```llvm
target triple = "x86_64-unknown-linux-gnu"

declare void @llvm.memset.p0.i64(ptr nocapture writeonly, i8, i64, i1) nounwind

define void @memset_nontemporal(ptr %dst) {
entry:
  call void @llvm.memset.p0.i64(ptr align 16 %dst, i8 0, i64 32, i1 false),
       !nontemporal !0
  ret void
}

!0 = !{i32 1}
```

## MIR (`llc -O2 -mtriple=x86_64-unknown-linux-gnu -stop-after=finalize-isel`)
Actual (buggy):
```
body:             |
  bb.0.entry:
    liveins: $rdi
    %0:gr64 = COPY $rdi
    %1:vr128 = V_SET0
    MOVAPSmr %0, 1, $noreg, 16, $noreg, %1 :: (store (s128) into %ir.dst + 16)
    MOVAPSmr %0, 1, $noreg, 0, $noreg, %1 :: (store (s128) into %ir.dst)
    RET 0
```
The MMOs are `(store (s128) ...)` instead of `(non-temporal store ...)`,
and the back end therefore selects `MOVAPSmr` rather than `MOVNTPSmr` /
`MOVNTDQmr`. A direct `store <16 x i8> zeroinitializer, … !nontemporal`
lowers to NT correctly (see w460 baseline), confirming the wiring exists
and only this intrinsic path misses it.

## Why it matters
This is exactly the use case the NT bit was designed for — bulk page /
buffer zeroing where you do not want the destination to pollute the
cache. Today the compiler quietly degrades to a non-NT memset.

## Fix sketch
Thread `MachineMemOperand::Flags` through `getMemset` →
`getMemsetStores` and OR it into the per-store flag computation at
SelectionDAG.cpp:9751. In `SelectionDAGBuilder` for
`Intrinsic::memset`, mirror the existing NT handling on `StoreInst`
(line 5119-5120) and forward the bit.
