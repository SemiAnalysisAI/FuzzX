# w370: X86 LowerLoad/LowerStore for vXi1 (no DQI) drops AAMDNodes and Ranges

## Summary

`X86TargetLowering::LowerLoad` and `LowerStore` rebuild the load/store as an
`i8`-sized memory op for `v2i1`/`v4i1`/`v8i1` (AVX512F without AVX512DQI). They
call `DAG.getLoad`/`DAG.getStore` with `PtrInfo`+`Align`+`MMOFlags` but pass no
`AAMDNodes` and no `Ranges`. The new `MachineMemOperand` ends up with **default
(empty) AAInfo and null Ranges**, while the original LLVM IR load/store had
`!alias.scope` / `!noalias` (and could have `!nontemporal_load`, `!range`, etc.
for similar legalizers).

The bare MMO flags (`MOInvariant`, `MONonTemporal`, `MODereferenceable`) ARE
preserved because they live in `getMemOperand()->getFlags()`. What is lost is
the AA metadata (`alias.scope`/`noalias`), which Machine-IR alias analysis uses
to reorder memory ops in later passes (MachineSink, MachineLICM,
MachineScheduler, etc.).

## Source

```
llvm/lib/Target/X86/X86ISelLowering.cpp:26437-26439
    SDValue NewLd = DAG.getLoad(MVT::i8, dl, Ld->getChain(), Ld->getBasePtr(),
                                Ld->getPointerInfo(), Ld->getBaseAlign(),
                                Ld->getMemOperand()->getFlags());

llvm/lib/Target/X86/X86ISelLowering.cpp:26361-26363  (LowerStore i1-vec path)
    return DAG.getStore(St->getChain(), dl, StoredVal, St->getBasePtr(),
                        St->getPointerInfo(), St->getBaseAlign(),
                        St->getMemOperand()->getFlags());
```

The relevant overloads (`SelectionDAG.h:1525-1531` and analogous for store) take
`MMOFlags = MONone` and `AAMDNodes & = AAMDNodes()` defaults. The X86 lowering
never reads `Ld->getAAInfo()` / `Ld->getRanges()` and never passes them through.

Compare to the *general* DAG-builder pattern in `SelectionDAGBuilder`, which
always plumbs AAInfo through. Compare also with elsewhere in this same file:
e.g. `LowerStore`'s 256/512-bit halving path uses the
`getStore(Chain,dl,Val,Ptr,MMO)` overload that copies the full MMO including
AAInfo. Only the vXi1 path drops it.

## Reproducer

`/tmp/x86h/i1load2.ll`:

```llvm
target triple = "x86_64-unknown-linux-gnu"

define void @load_i1_vec(ptr %p, ptr %q) {
entry:
  %v = load <8 x i1>, ptr %p, align 1, !alias.scope !2, !noalias !5
  store <8 x i1> %v, ptr %q, align 1, !alias.scope !5, !noalias !2
  ret void
}

!1 = !{!"scope_domain"}
!2 = !{!3}
!3 = distinct !{!3, !1, !"alias_scope_a"}
!5 = !{!6}
!6 = distinct !{!6, !1, !"alias_scope_b"}
```

`llc -O2 -mtriple=x86_64-unknown-linux-gnu -mattr=+avx512f -stop-after=finalize-isel`:

```
    %2:gr8 = MOV8rm %0, 1, $noreg, 0, $noreg :: (load (s8) from %ir.p)
    MOV8mr %1, 1, $noreg, 0, $noreg, killed %2 :: (store (s8) into %ir.q)
```

The `!alias.scope` / `!noalias` are **missing** from both MMOs.

Control: change `<8 x i1>` to plain `i8` (same alias metadata) and the MMOs are
correctly emitted as:

```
    %2:gr8 = MOV8rm %0, 1, $noreg, 0, $noreg :: (load (s8) from %ir.p, !alias.scope !0, !noalias !3)
    MOV8mr %1, 1, $noreg, 0, $noreg, killed %2 :: (store (s8) into %ir.q, !alias.scope !3, !noalias !0)
```

## Impact

- Machine-IR alias analysis cannot use the dropped scope info, missing
  legitimate reorderings (perf, not correctness).
- For non-DQI AVX512F targets only (covers AVX512F-only chips like KNL and
  models that disable DQI via `-mattr=-avx512dq`). Loads/stores of
  `<{2,4,8} x i1>` are affected.
- Same root cause applies to `LowerStore` i1-vec path (26361-26363).

## Severity

Low-to-medium. No observable correctness issue, but a real metadata leak that
silently disables downstream optimizations. The fix is mechanical: pass
`Ld->getAAInfo()` / `Ld->getRanges()` (and the symmetric `St->getAAInfo()`) to
the `getLoad`/`getStore` calls, or switch to the `MachineMemOperand*`-taking
overload.
