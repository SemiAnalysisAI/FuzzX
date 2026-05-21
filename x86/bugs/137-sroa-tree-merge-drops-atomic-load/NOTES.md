# w78 -- SROA `rewriteTreeStructuredMerge` drops atomic ordering on the load

## Component

`llvm/lib/Transforms/Scalar/SROA.cpp`, the `rewriteTreeStructuredMerge`
helper (lines 2853-3035).

The eligibility test for the load only filters volatile (line 2904):

```cpp
if (TheLoad || !IsTypeValidForTreeStructuredMerge(LI->getType()) ||
    S.beginOffset() != NewAllocaBeginOffset ||
    S.endOffset() != NewAllocaEndOffset || LI->isVolatile())
  return std::nullopt;
```

An atomic-but-not-volatile load passes through. The rewrite then creates a
plain `CreateAlignedLoad` (lines 3028-3031):

```cpp
IRBuilder<> LoadBuilder(TheLoad);
TheLoad->replaceAllUsesWith(LoadBuilder.CreateAlignedLoad(
    TheLoad->getType(), &NewAI, getSliceAlign(), TheLoad->isVolatile(),
    TheLoad->getName() + ".sroa.new.load"));
```

`TheLoad->isVolatile()` is false, so the new load is non-volatile, and the
helper never propagates `setAtomic(ordering, scope)` -- the atomic ordering
of the original load is silently discarded. (Sibling path: w61 catches the
analogous bug in the *normal* slice rewriter; this one is the
tree-structured-merge variant.)

The store-side has the same shape: line 2915 only checks `isVolatile()`, and
line 3026 calls `CreateAlignedStore` with no atomic propagation.

## Reproducer

`/tmp/w78/sroa_tree_atomic_load.ll`:

```ll
target triple = "x86_64-unknown-linux-gnu"
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"

define <8 x float> @tree_atomic_load(<4 x float> %lo, <4 x float> %hi) {
  %a = alloca <8 x float>, align 32
  store <4 x float> %lo, ptr %a, align 16
  %p1 = getelementptr i8, ptr %a, i64 16
  store <4 x float> %hi, ptr %p1, align 16
  %ld = load atomic <8 x float>, ptr %a unordered, align 32
  ret <8 x float> %ld
}
```

## opt diff

`opt -passes=sroa` rewrites the function to:

```ll
define <8 x float> @tree_atomic_load(<4 x float> %lo, <4 x float> %hi) {
  %1 = shufflevector <4 x float> %lo, <4 x float> %hi, <8 x i32> ...
  ret <8 x float> %1
}
```

The `load atomic <8 x float> ... unordered` is replaced by a plain shuffle:
the synthesized new alloca load has no atomic ordering and is then
promoted away.

## llc diff (x86_64)

Before SROA: emits a call to `__atomic_load@PLT` for the wide atomic load.

After SROA: the function becomes a trivial shuffle/return -- the call to
`__atomic_load` disappears.

```
# before SROA
movaps  %xmm0, (%rsp)
movaps  %xmm1, 16(%rsp)
movq    %rsp, %rsi
leaq    48(%rsp), %rdx
movl    $32, %edi
xorl    %ecx, %ecx
callq   __atomic_load@PLT
movaps  48(%rsp), %xmm0
movaps  64(%rsp), %xmm1
...

# after SROA
retq
```

## Caveat

The alloca is local and not address-taken, so the LLVM langref allows
folding the atomic load to a non-atomic load. The "miscompile" is therefore
benign in this exact form, but the same structural bug -- a code path that
checks `isVolatile()` and forgets `isAtomic()` -- becomes a real miscompile
the moment the alloca's address escapes (e.g. through capture, see w61 for
the analogous escape in the normal rewriter).

## Fix sketch

The eligibility filter at lines 2902-2916 should reject `isAtomic()` loads
and stores, or the rewriter at 3026/3029 should propagate the atomic
ordering with `setAtomic(getOrdering(), getSyncScopeID())` and re-apply the
original alignment, mirroring the pattern in lines 3166-3169 for the normal
slice rewriter.
