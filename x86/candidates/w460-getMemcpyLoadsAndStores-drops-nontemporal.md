# w460: SelectionDAG::getMemcpyLoadsAndStores drops !nontemporal

## Summary
`llvm.memcpy` with `!nontemporal` metadata, when expanded inline to a
sequence of loads and stores, has its non-temporal hint silently dropped:
the resulting MMOs only carry `MOVolatile` (and the dereferenceability /
invariant flags), never `MONonTemporal`. So the user-visible request for
NT-streaming stores is lost and the back end emits plain MOVAPS instead
of MOVNT*.

## Root cause (cite)
`llvm/lib/CodeGen/SelectionDAG/SelectionDAG.cpp`
- Line 9331-9332 (inside `getMemcpyLoadsAndStores`):
  ```cpp
  MachineMemOperand::Flags MMOFlags =
      isVol ? MachineMemOperand::MOVolatile : MachineMemOperand::MONone;
  ```
  The flags word is built solely from `isVol`. There is no source for
  `MONonTemporal` in this function; the caller `getMemcpy` (line 9888)
  receives a `const CallInst *CI` but only uses it for tail-call hints
  in the libcall fallback, never to extract `!nontemporal`.

`llvm/lib/CodeGen/SelectionDAG/SelectionDAGBuilder.cpp`
- Line 6711-6715 (visitIntrinsicCall, `Intrinsic::memcpy`):
  ```cpp
  SDValue MC = DAG.getMemcpy(Root, sdl, Dst, Src, Size, Alignment, isVol,
                             MCI.isForceInlined(), &I, std::nullopt,
                             MachinePointerInfo(I.getArgOperand(0)),
                             MachinePointerInfo(I.getArgOperand(1)),
                             I.getAAMetadata(), BatchAA);
  ```
  The builder passes `&I` so the helper *could* check
  `I.hasMetadata(LLVMContext::MD_nontemporal)`, but does not. The same
  builder, for ordinary `LoadInst`/`StoreInst`, *does* propagate NT
  (`SelectionDAGBuilder.cpp:4961-4962` and `5119-5120`).

So the propagation is wired up for ordinary loads/stores and forgotten
for the memcpy intrinsic family. The same defect lives in
`getMemmoveLoadsAndStores` (line 9520-9521) and `getMemsetStores`
(line 9747-9751) — see sibling candidates w461 and w462.

## .ll
```llvm
target triple = "x86_64-unknown-linux-gnu"

declare void @llvm.memcpy.p0.p0.i64(ptr noalias nocapture writeonly,
                                    ptr noalias nocapture readonly,
                                    i64, i1) nounwind

define void @memcpy_nontemporal(ptr noalias %dst, ptr noalias %src) {
entry:
  call void @llvm.memcpy.p0.p0.i64(ptr align 16 %dst, ptr align 16 %src,
                                   i64 32, i1 false), !nontemporal !0
  ret void
}

!0 = !{i32 1}
```

## MIR (`llc -O2 -mtriple=x86_64-unknown-linux-gnu -stop-after=finalize-isel`)
Actual (buggy):
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
Note the stores are plain `MOVAPSmr` with `:: (store (s128) into %ir.dst...)`
— no `non-temporal` flag word in either the opcode or the MMO.

Expected (matching the direct `store … !nontemporal` baseline):
the destination stores should be tagged `:: (non-temporal store (s128) ...)`,
which would let the X86 back end pick `MOVNTPSmr` / `MOVNTDQmr`. The
baseline for an ordinary `store … !nontemporal !0` lowers to:
```
MOVNTPSmr %0, 1, $noreg, 0, $noreg, %1 :: (non-temporal store (s128) into %ir.p)
```
demonstrating the X86 selector is fully wired for NT stores once the
MMO carries the bit.

## Why it matters
NT-memcpy is a real-world pattern for streaming/write-combining copies in
high-bandwidth code (NUMA initialisers, video frame fillers, OS page
zeroers). After this lowering, the cache-pollution-avoiding behaviour the
user asked for is silently dropped; behaviour is functionally correct
(observable result is the same) but performance and cache state are wrong
and unobservable from the IR.

## Fix sketch
In `SelectionDAGBuilder::visitIntrinsicCall` for `memcpy`/`memmove`/`memset`,
compute `MMOFlags |= MachineMemOperand::MONonTemporal` when
`I.hasMetadata(MD_nontemporal)`, and thread an extra
`MachineMemOperand::Flags` parameter through `getMemcpy`/`getMemmove`/
`getMemset` into the three load/store helpers so it can be OR'd into the
flags computed at SelectionDAG.cpp:9331, 9520, 9751.
