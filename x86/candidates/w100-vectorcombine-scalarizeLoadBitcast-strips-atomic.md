# w100 - VectorCombine `scalarizeLoadBitcast` drops `atomic` on vector load

## Location

`llvm/lib/Transforms/Vectorize/VectorCombine.cpp`

- Entry gate: `VectorCombine::scalarizeLoad` line 2008
- Defective path: `VectorCombine::scalarizeLoadBitcast` line 2150

The entry at line 2015 only filters out `isVolatile`:

```cpp
if (LI->isVolatile() || !DL->typeSizeEqualsStoreSize(VecTy->getScalarType()))
  return false;
```

It does NOT check `LI->isAtomic()`. An `atomic unordered <N x T>` load that
feeds only bitcast users falls through to `scalarizeLoadBitcast`. Line 2202
then creates a NON-atomic scalar load:

```cpp
auto *ScalarLoad =
    Builder.CreateLoad(TargetScalarType, Ptr, LI->getName() + ".scalar");
ScalarLoad->setAlignment(LI->getAlign());
ScalarLoad->copyMetadata(*LI);   // metadata only - DOES NOT carry ordering
```

`Builder.CreateLoad` and `copyMetadata` never set the AtomicOrdering /
SyncScope from the original `LoadInst`. The output is a plain `load i64`.

This is the same defect class as bug `w63b` for `scalarizeLoadExtract`
(extract path) — that path was patched (or partly so); the bitcast path was
not. `unordered` atomic is silently downgraded to non-atomic.

## Repro (`repro.ll`)

```llvm
target triple = "x86_64-unknown-linux-gnu"
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"

define i64 @atomic_load_bitcast(ptr %p) {
  %v  = load atomic <2 x i32>, ptr %p unordered, align 8
  %bc = bitcast <2 x i32> %v to i64
  ret i64 %bc
}
```

## Invocation

```
opt -mtriple=x86_64 -passes=vector-combine -S repro.ll
```

## Observed `opt` output

```
define i64 @atomic_load_bitcast(ptr %p) {
  %bc = load i64, ptr %p, align 8         ; <-- atomic ordering GONE
  ret i64 %bc
}
```

Original was `load atomic ... unordered`; the produced IR is a plain
non-atomic `load`. The vector-combine pass has emitted IR with a strictly
weaker memory-model guarantee than its input.

## Why this matters on x86

For this exact i64 case the codegen happens to be the same (`movq (%rdi),
%rax`) because aligned 8-byte loads on x86_64 are already single-instruction
atomic. The bug bites when:
- The load type is `<4 x i8>` / `<2 x i16>` / `i128` etc. where the original
  atomic guarantees no-torn-read but the scalarized non-atomic version
  permits the codegen to split (LICM/LSR/load-store-vectorizer downstream
  may further split or merge with non-atomics).
- Optimizer-level transforms that key on `isAtomic()` (e.g. LICM hoisting,
  GVN load-store forwarding, MemorySSA aliasing) now consider this load as
  ordinary memory access, which is incorrect for the source program.
- Any later pass running between vector-combine and the next instcombine
  observes the wrong IR.

## Pipeline note (-O2)

At `-O2`, instcombine's bitcast-of-load canonicalizer often runs first and
converts the input to a scalar atomic load before vector-combine sees it, so
this bug stays hidden for many inputs. It is observable whenever
vector-combine sees the `bitcast (load atomic <N x T>)` shape — which
includes the dedicated -passes=vector-combine path and any -O2 input where
instcombine declined to canonicalize (e.g. due to multi-use bitcast or
pre-canonicalized IR from earlier passes).

## Fix

Tighten the entry gate at line 2015 to require `LI->isSimple()` (matches the
existing test in `scalarizeLoad` companion code and `shrinkLoadForShuffles`
at line 5544):

```cpp
if (!LI->isSimple() || !DL->typeSizeEqualsStoreSize(...))
  return false;
```

Splitting a single atomic load into a single scalar atomic load of a
different type is itself questionable for orderings other than `unordered`
(it changes which other atomics it can be reordered with), so the simple
fix is to refuse the transform on any atomic.
