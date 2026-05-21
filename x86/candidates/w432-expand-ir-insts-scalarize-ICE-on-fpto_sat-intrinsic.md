# w432: ExpandIRInsts `scalarize()` ICEs on vector `llvm.fptoui.sat`/`llvm.fptosi.sat` intrinsics

## Where

`llvm/lib/CodeGen/ExpandIRInsts.cpp:1117-1149` — the `scalarize` helper recognizes only `BinaryOperator` and `CastInst`:

```cpp
if (auto *BinOp = dyn_cast<BinaryOperator>(I))
  NewOp = Builder.CreateBinOp(
      BinOp->getOpcode(), Ext, ...);
else if (auto *CastI = dyn_cast<CastInst>(I))
  NewOp = Builder.CreateCast(CastI->getOpcode(), Ext,
                             I->getType()->getScalarType());
else
  llvm_unreachable("Unsupported instruction type");
```

But the worklist is fed by `addToWorklist` (line 1151) which scalarizes *any* expandable instruction with a vector operand-0, and `ShouldHandleInst` adds `IntrinsicInst`s for `Intrinsic::fptoui_sat` / `Intrinsic::fptosi_sat` (lines `1213-1218`). For a vector intrinsic such as `<2 x i256> @llvm.fptoui.sat.v2i256.v2f32(<2 x float>)` neither `BinaryOperator` nor `CastInst` matches; this trips the `llvm_unreachable` (in release builds, this is whatever `LLVM_BUILTIN_UNREACHABLE` lowers to, which on the fuzzer build leads to an abort during ISel-time pass execution — observed as a hard crash from `expand-ir-insts`).

## Repro

`.ll` (`/home/orenamd@semianalysis.com/FuzzX/x86/scratch_w430/vec_sat.ll`):

```llvm
target triple = "x86_64-unknown-linux-gnu"
define <2 x i256> @vec_sat(<2 x float> %x) {
  %r = call <2 x i256> @llvm.fptoui.sat.v2i256.v2f32(<2 x float> %x)
  ret <2 x i256> %r
}
declare <2 x i256> @llvm.fptoui.sat.v2i256.v2f32(<2 x float>)
```

```
$ llc -O2 -mtriple=x86_64-unknown-linux-gnu vec_sat.ll
PLEASE submit a bug report ...
Stack dump:
0. Program arguments: llc -O2 -mtriple=x86_64-unknown-linux-gnu vec_sat.ll
1. Running pass 'Function Pass Manager' on module 'vec_sat.ll'.
2. Running pass 'Expand IR instructions' on function '@vec_sat'
...
```

(Crash reproduces at `-O0` too; the pass runs unconditionally for any expandable type.)

The crash frames show the abort emanating from inside the pass — `getCastOpcode` is reached because `dyn_cast<CastInst>` on the intrinsic is `nullptr`, but the codegen path that reaches the abort goes through the intrinsic's `Instruction` handling. The end result is the same: an unsupported instruction kind reaches `scalarize` and the pass crashes.

## Why it's a real bug

The dispatcher in `runImpl` (lines `1187-1224`) explicitly enumerates `Call` (the intrinsic case) as expandable. The scalarization helper is therefore reachable for an instruction kind it doesn't handle. Frontends generating `llvm.fptoui.sat.v<N>i256.v<N>f32` (legal IR per LangRef — `fptoui.sat` is overloaded on vector types) crash the backend instead of getting a per-lane expansion.

## Fix sketch

Extend `scalarize` with an `IntrinsicInst` arm that, for the two saturating intrinsics, calls `Builder.CreateIntrinsic(I->getType()->getScalarType(), II->getIntrinsicID(), {Ext}, ...)`. Alternatively, do the scalarization inside the expansion driver after the per-lane intrinsic call is recognized.

## Candidate-level confidence

High — clean crash with deterministic input, fully attributable to `expand-ir-insts`, with a clear missing case in the dispatcher.
