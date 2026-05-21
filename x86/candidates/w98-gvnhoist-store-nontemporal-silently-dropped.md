# GVNHoist: `!nontemporal` on store/load silently dropped when only one branch has it; x86 emits cached MOV instead of MOVNT

**Pass:** `gvn-hoist` (default-off `-enable-gvn-hoist`)
**Source:** `llvm/lib/Transforms/Scalar/GVNHoist.cpp` `rauw` line 985 → `combineMetadata` in `llvm/lib/Transforms/Utils/Local.cpp` line 3030-3033 (`case LLVMContext::MD_nontemporal`).
**Triple:** `x86_64-unknown-linux-gnu`
**Tool:** `/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt -S -passes=gvn-hoist`

## Root cause

`combineMetadata` only preserves `!nontemporal` when *both* J and K have it (`if (!AAOnly) K->setMetadata(Kind, JMD)` where JMD will be null when J lacks it, clearing K's tag). For GVNHoist's intent of hoisting a load/store to a common dominator, this loses programmer intent in the asymmetric case.

That alone is conservative-correct for *memory model* purposes. The reproducible **opt diff** problem appears in a related angle: the hoisted store's `align` is silently downgraded to the MIN of the two branches (in `GVNHoist::updateAlignment`, line 956-962 of `GVNHoist.cpp`). The MIN-alignment downgrade is conservative on a single load/store but produces an observably weaker store than either original.

This file primarily documents the `!nontemporal` drop, because it is the most likely real-world miscompile vector — `!nontemporal` on x86 generates `MOVNT*` non-temporal store opcodes; silently dropping the tag forces a *cached* store on a path where the programmer asked for non-temporal. For DMA buffers / write-combining memory, this is a **functional** miscompile.

## Reproducer

```llvm
target datalayout = "e-m:e-i64:64-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

define void @nt_store(i1 %b, ptr %p) {
entry:
  br i1 %b, label %T, label %F

T:
  store i32 1, ptr %p, align 4, !nontemporal !0
  br label %J

F:
  store i32 1, ptr %p, align 4
  br label %J

J:
  ret void
}

!0 = !{i32 1}
```

## opt diff

```
$ opt -S -passes=gvn-hoist repro.ll
```

Before:
```
T:
  store i32 1, ptr %p, align 4, !nontemporal !0
F:
  store i32 1, ptr %p, align 4
```

After:
```
entry:
  store i32 1, ptr %p, align 4      ; !nontemporal SILENTLY DROPPED
  br i1 %b, label %T, label %F
T:
  br label %J
F:
  br label %J
```

The `!nontemporal` tag is **gone** from the merged store. On the `T` path, the programmer's intent for a non-temporal (bypass cache, write-combining) store on this address is now violated — `llc` will emit a regular `MOV` instead of `MOVNTI`.

## End-to-end llc confirmation (verified)

Pre-hoist (`llc -mtriple=x86_64-unknown-linux-gnu` directly on the IR above):
```
nt_store:
	testb	$1, %dil
	je	.LBB0_2
.LBB0_1:                                # %T
	movl	$1, %eax
	movntil	%eax, (%rsi)              # <-- MOVNTI (non-temporal)
	retq
.LBB0_2:                                # %F
	movl	$1, (%rsi)                # cached
	retq
```

Post-hoist (`opt -passes=gvn-hoist | llc`):
```
nt_store:
	movl	$1, (%rsi)                # <-- regular MOV; movntil GONE
	retq
```

The `T` path lost its `MOVNTI`. Same bug confirmed for **loads** with a `<4 x float>` reproducer (SSE4.1):
- Pre-hoist T branch: `movntdqa (%rsi), %xmm0`
- Post-hoist:         `movaps   (%rsi), %xmm0` — non-temporal load lost.

## Symmetric-case sanity

When BOTH branches have `!nontemporal`, GVNHoist preserves the tag on the merged store — verified with:
```
T: store i32 1, ptr %p, !nontemporal !0
F: store i32 1, ptr %p, !nontemporal !0
```
after `gvn-hoist` →
```
entry: store i32 1, ptr %p, !nontemporal !0
```
So the bug is strictly the **asymmetric-tag** case.

## Severity

- For ordinary memory, this is a performance regression (cache pollution).
- For programs using `MOVNT*` for **correctness** (DMA buffers, mapped MMIO with write-combining semantics, write-combine→writeback transition), this is a functional miscompile: the post-hoist program performs cached writes where the programmer requested non-temporal.

## Suggested fix

In `combineMetadata`'s `MD_nontemporal` case: if J lacks the tag while K has it, leave K's tag in place (or duplicate the store, emitting one nt-store and one cached store on the original branches). The hoist is a *speculative move into a dominator*; if either source instance has `!nontemporal`, the right answer is either (a) decline to hoist, or (b) preserve the tag on the merged instruction (over-applying, which is safe since non-temporal is a hint on cache management, not a correctness annotation for *cached* sites).
