# 012 — CodeGenPrepare splits an **atomic** i64 store into two non-atomic i32 stores

Component: CodeGenPrepare (`splitMergedValStore`)

## Source

`llvm/lib/CodeGen/CodeGenPrepare.cpp`, `splitMergedValStore`
(around lines 8568–8665):

```cpp
if (SI.isVolatile() || ...)
  return false;
...
auto CreateSplitStore = [&](Value *V, bool Upper) {
  V = Builder.CreateZExtOrBitCast(V, SplitStoreType);
  ...
  Builder.CreateAlignedStore(V, Addr, Alignment);     // <-- no setAtomic call
};
CreateSplitStore(LValue, false);
CreateSplitStore(HValue, true);
SI.eraseFromParent();
```

The bail-out checks **only** `SI.isVolatile()` (line ~8590). `SI.isAtomic()`
is never consulted. When the i64 value being stored matches the pattern
`or(zext(lo), shl(zext(hi), 32))` and the (lo, hi) parts come from
mixed FP/int sources, `isMultiStoresCheaperThanBitsMerge` returns true and
the pass rewrites:

```
store atomic i64 %merged, ptr %p seq_cst, align 8
```

into

```
store i32 %lo,    ptr %p,        align 8        ; <-- NOT atomic
store i32 %hi_i,  ptr %p+4,      align 4        ; <-- NOT atomic
```

— two ordinary stores with no atomic ordering at all. The release/acquire
edge the user requested is silently discarded, and observers on other
threads can see torn values mid-update.

## Reproduction

`repro.ll` (the `atom_fp` function):

```ll
define void @atom_fp(ptr %p, i32 %lo, float %hi) {
  %hi_i = bitcast float %hi to i32
  %lo64 = zext i32 %lo to i64
  %hi64 = zext i32 %hi_i to i64
  %hishl = shl i64 %hi64, 32
  %merged = or i64 %lo64, %hishl
  store atomic i64 %merged, ptr %p seq_cst, align 8
  ret void
}
```

After CGP (via `llc -stop-after=codegenprepare`):

```
define void @atom_fp(ptr %p, i32 %lo, float %hi) {
  %hi_i = bitcast float %hi to i32
  ...
  store i32 %lo,   ptr %p,  align 8
  %1 = getelementptr i32, ptr %p, i32 1
  store i32 %hi_i, ptr %1,  align 4         ; ATOMICITY LOST
  ret void
}
```

Final asm for `atom_fp`:

```
atom_fp:
        movl    %esi, (%rdi)              ; plain mov, not lock-prefixed
        movd    %xmm0, 4(%rdi)            ; plain mov
        retq
```

A correct lowering of `store atomic i64 %merged, ptr %p seq_cst, align 8`
on x86_64 is a single 8-byte store (or, with `xchg` for `seq_cst`, an `xchg
%reg, (%rdi)`). The two ordinary 32-bit movs do not provide atomicity *or*
the seq_cst ordering.

## Why this is a wrong-code bug

Per LangRef on `store atomic ... seq_cst`:
- the access must be performed as a single atomic operation of the given
  size, and
- the seq_cst ordering establishes a global total order over all seq_cst
  operations.

Replacing it with two non-atomic stores violates both invariants. Another
thread doing a corresponding `load atomic i64 ... seq_cst` may observe
tearing (low half updated, high half stale).

## Fix

Change the bail-out to:

```cpp
if (SI.isVolatile() || SI.isAtomic())
  return false;
```

Or, propagate atomicity onto the two split stores — but that doesn't
preserve atomicity *between* them, so bailing is the correct fix.

## Files
- `repro.ll`  — both atomic and non-atomic versions, side by side
- `cmd.sh`    — dumps IR after CGP + final asm; the split is visible in both
