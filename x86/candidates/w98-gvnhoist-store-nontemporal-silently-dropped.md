# GVNHoist: `!nontemporal` on store silently dropped when only one branch has it; remains on neither branch post-hoist

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

## Confirmation

Compiling the post-hoist IR with `llc -mtriple=x86_64`:
```
mov dword ptr [rdi], 1
```

Same IR pre-hoist (without GVNHoist) compiles to `MOVNTI` on the T branch:
```
movnti dword ptr [rdi], eax
```

The opt path with `-enable-gvn-hoist` thus causes x86 codegen to *silently switch from non-temporal to cached* store on the path that had `!nontemporal`.

## Severity

- For ordinary memory, this is a performance regression (cache pollution).
- For programs using `MOVNT*` for **correctness** (DMA buffers, mapped MMIO with write-combining semantics, write-combine→writeback transition), this is a functional miscompile: the post-hoist program performs cached writes where the programmer requested non-temporal.

## Suggested fix

In `combineMetadata`'s `MD_nontemporal` case: if J lacks the tag while K has it, leave K's tag in place (or duplicate the store, emitting one nt-store and one cached store on the original branches). The hoist is a *speculative move into a dominator*; if either source instance has `!nontemporal`, the right answer is either (a) decline to hoist, or (b) preserve the tag on the merged instruction (over-applying, which is safe since non-temporal is a hint on cache management, not a correctness annotation for *cached* sites).
