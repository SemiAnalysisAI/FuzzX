# w223: DAGCombiner ReduceLoadOpStoreWidth drops MMO flags and AAInfo on narrowed store

## Summary

`DAGCombiner::ReduceLoadOpStoreWidth` matches the pattern
`store (op (load p), imm) p` and narrows it to a single
`store (op (load p+k), narrow_imm) p+k` whose width is the smallest
power-of-two that covers all of `imm`'s touched bytes.

The new narrow load is built correctly with `LD->getMemOperand()->getFlags()`
and `LD->getAAInfo()`. The new narrow store, however, uses the four-argument
`DAG.getStore(...)` overload, which defaults `MMOFlags = MONone` and
`AAInfo = {}`.

The asymmetric behaviour is plainly visible in the final MIR: the load MMO
retains `!tbaa` and `non-temporal`, the store MMO retains neither.

## Source

File: `llvm/lib/CodeGen/SelectionDAG/DAGCombiner.cpp`
- `ReduceLoadOpStoreWidth` lines 22441-22450:

```cpp
SDValue NewLD =
    DAG.getLoad(NewVT, SDLoc(N0), LD->getChain(), NewPtr,
                LD->getPointerInfo().getWithOffset(PtrOff), NewAlign,
                LD->getMemOperand()->getFlags(), LD->getAAInfo());
SDValue NewVal = DAG.getNode(Opc, SDLoc(Value), NewVT, NewLD,
                             DAG.getConstant(NewImm, SDLoc(Value), NewVT));
SDValue NewST =
    DAG.getStore(Chain, SDLoc(N), NewVal, NewPtr,
                 ST->getPointerInfo().getWithOffset(PtrOff), NewAlign);
```

The load passes `Flags` and `AAInfo`; the store does not.

## Reproducer

```ll
; /tmp/w220/reduce-loadop-store.ll
target triple = "x86_64-unknown-linux-gnu"

define void @t(ptr %p) {
  %v = load i32, ptr %p, align 4, !tbaa !0, !nontemporal !10
  %or = or i32 %v, 256          ; touches only byte 1
  store i32 %or, ptr %p, align 4, !tbaa !0, !nontemporal !10
  ret void
}

!0  = !{!1, !1, i64 0}
!1  = !{!"int", !2}
!2  = !{!"omnipotent char", !3}
!3  = !{!"Simple C/C++ TBAA"}
!10 = !{i32 1}
```

## Repro command

```
llc -mtriple=x86_64-unknown-linux-gnu -O2 \
    -stop-after=finalize-isel /tmp/w220/reduce-loadop-store.ll -o -
```

## MIR diff

```
OR8mi %0, 1, $noreg, 1, $noreg, 1, implicit-def dead $eflags
  :: (store (s8) into %ir.p + 1),
     (non-temporal load (s8) from %ir.p + 1, !tbaa !0)
```

The folded memory operand has two MMOs (one read, one write):

- Load side keeps `non-temporal` and `!tbaa !0`. (Correct, derived from
  the new narrow load that forwarded both.)
- Store side has no `non-temporal`, no `!tbaa`. (Wrong; should match the
  source `store i32 ..., ptr %p, align 4, !tbaa !0, !nontemporal !10`.)

## Severity

The `nontemporal` hint asks the hardware not to pollute caches with this
store. Dropping it changes observable cache behavior. The `!tbaa` drop also
weakens AA disambiguation for post-isel passes.

## Suggested fix

Replace the four-arg `getStore` call with the eight-arg form that takes
`MMOFlags` and `AAInfo`, mirroring the load:

```cpp
DAG.getStore(Chain, SDLoc(N), NewVal, NewPtr,
             ST->getPointerInfo().getWithOffset(PtrOff), NewAlign,
             ST->getMemOperand()->getFlags(), ST->getAAInfo());
```
