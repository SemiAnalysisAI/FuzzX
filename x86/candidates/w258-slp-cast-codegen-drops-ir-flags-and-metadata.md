# w258: SLPVectorizer cast codegen drops IR flags (`nneg`, `nuw`/`nsw` on trunc, `disjoint` style flags) and metadata

## Pass
`-passes=slp-vectorizer` (default x86 -O2 pipeline includes this).

## Summary

The `Instruction::ZExt` / `SExt` / `FPToUI` / `FPToSI` / `FPExt` / `PtrToInt` /
`IntToPtr` / `SIToFP` / `UIToFP` / `Trunc` / `FPTrunc` / `BitCast` case in
`BoUpSLP::vectorizeTree` (SLPVectorizer.cpp:22869-22928) creates the vector
cast via `Builder.CreateCast(VecOpcode, InVec, VecTy)` and then immediately
returns. It calls neither `propagateIRFlags` nor `::propagateMetadata`.

That means every per-cast IR flag is dropped on vectorization:

- `zext nneg` → `zext`
- `trunc nuw nsw` → `trunc`
- `uitofp nneg` → `uitofp`

…even when every scalar in the bundle had the flag. These are the same flags
that the binop / fneg / shuffle / cmp paths take care to preserve via
`propagateIRFlags(V, E->Scalars, VL0)`.

Metadata such as `!fpmath` (on fp-cast), or future cast-attached metadata, is
also dropped.

## Source (LLVM 23.0.0git, `llvm/lib/Transforms/Vectorize/SLPVectorizer.cpp`)

```cpp
// 22869  case Instruction::ZExt:
// 22870  case Instruction::SExt:
// 22871  case Instruction::FPToUI:
// 22872  case Instruction::FPToSI:
// 22873  case Instruction::FPExt:
// 22874  case Instruction::PtrToInt:
// 22875  case Instruction::IntToPtr:
// 22876  case Instruction::SIToFP:
// 22877  case Instruction::UIToFP:
// 22878  case Instruction::Trunc:
// 22879  case Instruction::FPTrunc:
// 22880  case Instruction::BitCast: {
// ...
// 22920    Value *V = (VecOpcode != ShuffleOrOp && VecOpcode == Instruction::BitCast)
// 22921                   ? InVec
// 22922                   : Builder.CreateCast(VecOpcode, InVec, VecTy);
// 22923    V = FinalShuffle(V, E);
// 22924
// 22925    E->VectorizedValue = V;
// 22926    ++NumVectorInstructions;
// 22927    return V;
// 22928  }
```

No `propagateIRFlags(V, E->Scalars, VL0)` and no `::propagateMetadata(I, E->Scalars)`
between the `CreateCast` (22922) and `return V` (22927). For comparison the
FNeg case immediately above (line 23023-23039) does both.

## Reproducer A — `zext nneg` dropped

`t_zext_nneg.ll`:
```llvm
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

define void @f(ptr %p, ptr %q) {
  %p0 = getelementptr i64, ptr %p, i32 0
  %p1 = getelementptr i64, ptr %p, i32 1
  %q0 = getelementptr i32, ptr %q, i32 0
  %q1 = getelementptr i32, ptr %q, i32 1
  %a = load i32, ptr %q0
  %b = load i32, ptr %q1
  %za = zext nneg i32 %a to i64
  %zb = zext nneg i32 %b to i64
  store i64 %za, ptr %p0
  store i64 %zb, ptr %p1
  ret void
}
```

Command:
```
opt -passes=slp-vectorizer -S t_zext_nneg.ll
```

Output:
```llvm
  %1 = load <2 x i32>, ptr %q0, align 4
  %2 = zext <2 x i32> %1 to <2 x i64>       ; <-- nneg dropped
  store <2 x i64> %2, ptr %p0, align 8
```

Expected: `%2 = zext nneg <2 x i32> %1 to <2 x i64>` (every scalar zext had
`nneg`, intersection is `nneg`).

## Reproducer B — `trunc nuw nsw` dropped

```llvm
  %ta = trunc nuw nsw i32 %a to i16
  %tb = trunc nuw nsw i32 %b to i16
  %tc = trunc nuw nsw i32 %c to i16
  %td = trunc nuw nsw i32 %d to i16
```

Output:
```llvm
  %1 = load <4 x i32>, ptr %q0, align 4
  %2 = trunc <4 x i32> %1 to <4 x i16>       ; <-- nuw nsw dropped
  store <4 x i16> %2, ptr %p0, align 2
```

Expected: `%2 = trunc nuw nsw <4 x i32> %1 to <4 x i16>`.

A control test on the same code WITHOUT vectorization (single trunc) shows the
flags are normally preserved through other transforms — the loss is specific to
SLP's cast emission.

## Why this matters

- `nneg` on `zext` allows InstCombine to treat the zext as a `sext` for
  certain canonicalizations and lets backend instruction selection pick
  cheaper signed encodings. Dropping it forces the conservative unsigned
  treatment.
- `nuw`/`nsw` on `trunc` are inputs to value-range analysis (e.g.
  `KnownBits`, `LazyValueInfo`) and SCEV. Losing them blocks downstream
  loop-strength-reduction and InstCombine simplifications.
- The dropped flags survive across every other SLP-handled opcode; cast is
  the lone exception in the same file.

## Suggested fix

```cpp
Value *V = (VecOpcode != ShuffleOrOp && VecOpcode == Instruction::BitCast)
               ? InVec
               : Builder.CreateCast(VecOpcode, InVec, VecTy);
+if (auto *I = dyn_cast<Instruction>(V)) {
+  propagateIRFlags(V, E->Scalars, VL0);
+  V = ::propagateMetadata(I, E->Scalars);
+}
V = FinalShuffle(V, E);
```

(For `BitCast`-only paths where `V == InVec` we should not retroactively
attach flags to an unrelated existing instruction — the `dyn_cast<Instruction>`
guard handles the corner case where `V` is the original input.)
