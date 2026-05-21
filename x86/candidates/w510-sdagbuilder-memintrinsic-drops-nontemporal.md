# w510 - SelectionDAGBuilder lowers `!nontemporal` `llvm.memcpy/memmove/memset[.inline]` as ordinary (cached) loads/stores

## Location

`llvm/lib/CodeGen/SelectionDAG/SelectionDAGBuilder.cpp` -
`SelectionDAGBuilder::visitIntrinsicCall` cases
`Intrinsic::memcpy` / `Intrinsic::memcpy_inline` /
`Intrinsic::memmove` / `Intrinsic::memset` / `Intrinsic::memset_inline`
at lines 6695-6736 (and `visitMemPCpyCall` at 9473).

All five mem-intrinsic cases pull every other piece of information off
the call (alignment, volatile, AAMD, etc.) but never look at the call's
own `MD_nontemporal` metadata before handing the work off to
`SelectionDAG::getMemcpy` / `getMemmove` / `getMemset`. Inside
`SelectionDAG.cpp` the worker functions
`getMemcpyLoadsAndStores` / `getMemmoveLoadsAndStores` /
`getMemsetStores` build their MMO flags from `isVol` alone
(`MOVolatile` or `MONone`, lines 9331-9332 / 9521 / 9730-ish), so even
if the builder were willing to forward a `MONonTemporal` bit there is
no parameter to forward it through.

The asymmetry is glaring: regular `load`/`store` already get the bit
via `TargetLoweringBase::getLoadMemOperandFlags` /
`getStoreMemOperandFlags`
(`llvm/lib/CodeGen/TargetLoweringBase.cpp:2793-2794, 2821-2822`), and
`visitMaskedLoad`/`visitMaskedStore` check `MD_nontemporal` explicitly
(`SelectionDAGBuilder.cpp:4961-4962, 5119-5120`). Memory intrinsics
are the only memory-touching builders that silently drop the hint.

The bug also affects the FORCED-inline variants
(`memcpy_inline`, `memset_inline`) where the user has explicitly asked
the compiler NOT to fall back to a libcall and so has no other way of
keeping their cache-bypass hint.

## Repro

`memcpy_nt.ll`:

```llvm
target triple = "x86_64-unknown-linux-gnu"
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-f80:128-n8:16:32:64-S128"

declare void @llvm.memcpy.p0.p0.i64(ptr noalias nocapture writeonly,
                                     ptr noalias nocapture readonly,
                                     i64, i1 immarg)
declare void @llvm.memset.inline.p0.i64(ptr nocapture writeonly, i8,
                                         i64 immarg, i1 immarg)

define void @mcpy_nt(ptr noalias %dst, ptr noalias %src) {
  call void @llvm.memcpy.p0.p0.i64(ptr align 64 %dst, ptr align 64 %src,
                                    i64 128, i1 false), !nontemporal !0
  ret void
}

define void @mset_inline_nt(ptr %dst) {
  call void @llvm.memset.inline.p0.i64(ptr align 16 %dst, i8 0,
                                        i64 64, i1 false), !nontemporal !0
  ret void
}

!0 = !{ i32 1 }
```

For comparison, `store_nt.ll`:

```llvm
target triple = "x86_64-unknown-linux-gnu"

define void @str_nt(ptr %dst, <2 x i64> %v) {
  store <2 x i64> %v, ptr %dst, align 16, !nontemporal !0
  ret void
}

!0 = !{ i32 1 }
```

## Invocation

```
llc -O2 -mtriple=x86_64-unknown-linux-gnu -stop-after=finalize-isel memcpy_nt.ll
llc -O2 -mtriple=x86_64-unknown-linux-gnu memcpy_nt.ll        # asm
llc -O2 -mtriple=x86_64-unknown-linux-gnu store_nt.ll         # control
```

## Observed behaviour

The plain `store ... !nontemporal` correctly lowers as a
non-temporal MMO and emits `movntps`/`MOVNTDQmr`:

```
MOVNTDQmr %0, 1, $noreg, 0, $noreg, %1 :: (non-temporal store (s128) into %ir.dst)
# asm:  movntps %xmm0, (%rdi)
```

The `llvm.memcpy ... !nontemporal` is broken into a long sequence of
PLAIN, CACHED MOVAPSrm/MOVAPSmr pairs; no MMO carries the
`non-temporal` flag and the final asm contains zero `movnt*` opcodes:

```
%2:vr128 = MOVAPSrm %1, 1, $noreg, 112, $noreg :: (load (s128) from %ir.src + 112)
MOVAPSmr %0, 1, $noreg, 112, $noreg, killed %2 :: (store (s128) into %ir.dst + 112, basealign 64)
...
# asm:
#   movaps 112(%rsi), %xmm0
#   movaps %xmm0, 112(%rdi)
#   ...
```

The same is true of `llvm.memset.inline ... !nontemporal`: it lowers
to a sequence of cached `movaps %xmm0, N(%rdi)` instead of `movntps`.
A check of `llvm.memmove ... !nontemporal` shows identical behaviour.

## Why this is a (small) miscompile, not just a missed-optimization

`!nontemporal` is documented as a HINT, but the SDAG infrastructure
already honors the bit for plain load/store and for masked load/store -
so the user has a reasonable expectation that "I asked for streaming,
I got streaming". When the user asks the compiler to inline a
non-temporal copy (`llvm.memcpy.inline` / `llvm.memset.inline`,
specifically created to keep codegen under user control) and the
backend silently transforms it into a cached fill, the program's
observable cache footprint is wrong on every architecture that has
a non-temporal store form: subsequent reads of `%dst` no longer
miss in cache, eviction pressure is moved to the wrong working
set, and explicit measurements of L1/L2 miss rate disagree with the
source.

## Where the data is lost

1. `SelectionDAGBuilder.cpp:6711` (`memcpy`),
   `6731` (`memset`), `6750` (`memmove`),
   `9489` (mempcpy lowering):
   every call site of `DAG.getMemcpy / getMemset / getMemmove`
   passes the `CallInst*` but no `nontemporal` flag.
2. `SelectionDAG.cpp:9888-9967` (`getMemcpy`),
   `10003-...` (`getMemmove`), `10109-...` (`getMemset`):
   the public APIs take `isVol` but no `IsNonTemporal`, and
   the helper functions
   `getMemcpyLoadsAndStores` / `getMemmoveLoadsAndStores` /
   `getMemsetStores` build `MMOFlags` from `isVol` alone
   (`SelectionDAG.cpp:9331-9332, 9521-ish, 9730-ish`).

A natural fix is to (a) plumb `MachineMemOperand::MONonTemporal`
through `getMemcpy`/`getMemmove`/`getMemset` (mirroring the
existing `isVol` parameter), and (b) at the four
`visitIntrinsicCall` cases set the bit from
`MCI.hasMetadata(LLVMContext::MD_nontemporal)` /
`MSI.hasMetadata(...)`. The existing masked-load/store builders
already follow exactly that pattern at
`SelectionDAGBuilder.cpp:4961-4962, 5119-5120`.

## Relation to existing candidates

Distinct from the previously-filed mid-end drops
(`w76` memcpyopt-trymerge, `w105` simplifycfg-hoist,
`w53/w281` memcpyopt processMemCpy*): those bugs are about
optimization passes losing the metadata BEFORE codegen sees it.
The bug here is in codegen itself - even when the metadata
reaches the SDAG builder intact, it is discarded at the
instruction-selection boundary.
