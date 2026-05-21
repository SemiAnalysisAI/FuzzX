# m106: `SelectVOP3ModsImpl` folds `fsub +0.0, x` into VOP3 NEG src-modifier, dropping sign-of-zero

*Discovery method: code inspection.* Sibling shape to m094
(`fmul.legacy` sign-of-zero) but at the SDAG ISel layer rather than
InstCombine.

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/AMDGPUISelDAGToDAG.cpp:3415-3423`:

```cpp
} else if (Src.getOpcode() == ISD::FSUB && IsCanonicalizing) {
  // Fold fsub [+-]0 into fneg. This may not have folded depending on
  // the FP mode.
  auto *LHS = dyn_cast<ConstantFPSDNode>(Src.getOperand(0));
  if (LHS && LHS->isZero()) {              // <-- accepts +0.0 AND -0.0
    Mods |= SISrcMods::NEG;
    Src = Src.getOperand(1);
  }
}
```

`APFloat::isZero()` returns true for **both** `+0.0` and `-0.0`.  There
is no `nsz`/`hasNoSignedZeros()` check anywhere in the function.

Under IEEE 754, `fsub +0.0, x` is NOT equivalent to `fneg(x)` when
`x == +0.0`:

| expression | result for `x = +0.0` |
| --- | --- |
| `fsub +0.0, +0.0` | `+0.0` |
| `-(+0.0)`         | `-0.0` |

Folding `fsub +0.0, x` into the VOP3 `NEG` source modifier produces the
`fneg` semantics, so any kernel that exercises this path with `x = +0.0`
silently flips the sign-of-zero of the result.

The asymmetric form `fsub -0.0, x` IS algebraically `-x` for all `x`
including `±0`, so the right gate is either
`LHS->isNegZero()` or `Src->getFlags().hasNoSignedZeros()`.

## Reproducer

`reduced.ll`:

```llvm
define amdgpu_kernel void @t(ptr addrspace(1) %out, float %x, float %y) {
  %neg = fsub float 0.0, %x       ; <- fold fires: NEG modifier on %x
  %r   = fmul float %neg, %y      ; <- VOP3 consumer of NEG
  store float %r, ptr addrspace(1) %out
  ret void
}
```

Compile with `clang -mcpu=gfx950 -O2`:

```asm
v_mul_f32_e64 v1, -v1, v2          ; <-- fsub folded into NEG modifier
global_store_dword v0, v1, s[2:3]
```

Compare against GISel (`-mllvm -global-isel`):

```asm
v_sub_f32_e64 v0, 0, s2            ; preserves fsub
v_mul_f32_e32 v0, s3, v0
```

For `x = +0.0, y = +1.0`:

* IEEE / GISel: `fsub +0,+0 = +0`, then `*1 = +0` -> `0x00000000`.
* SDAG: `(-0) * 1 = -0` -> `0x80000000`.

## Why no runtime O0/O2 mismatch in the FuzzX harness

The fold is part of ISel complex-pattern matching, which runs at every
optimisation level.  Both `-O0` and `-O2` invoke the same buggy
matcher, so the FuzzX O0-vs-O2 oracle reports `any_mismatch=false`
even though both pipelines are wrong relative to IR semantics.

The witness is `SDAG vs GISel` or `SDAG vs IR semantics`.

## Suggested fix

Restrict the constant-zero check to negative zero (since `-0.0 - x` is
algebraically `-x` for every `x`):

```cpp
if (LHS && LHS->isNegZero()) {
  Mods |= SISrcMods::NEG;
  Src = Src.getOperand(1);
}
```

Or, more permissively, gate on `nsz`:

```cpp
if (LHS && LHS->isZero() && Src->getFlags().hasNoSignedZeros()) { ... }
```

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Reproduces (`v_mul_f32_e64 v1, -v1, v2`). |
| ROCm 7.1.1 (`/opt/rocm-7.1.1/lib/llvm/bin/clang-20`) | Same fold present. |

Not a HEAD-only regression -- the fold has been in
`SelectVOP3ModsImpl` since the source-modifier matcher landed.

## Why the fuzzer hasn't caught it

* The FP emitter generates plenty of `fsub C, x` shapes but rarely
  with `x` simultaneously being `+0.0` and feeding a VOP3 consumer
  in the same trace.
* The O0-vs-O2 oracle is blind because both pipelines run the same
  buggy ISel matcher.
* Per `MEMORY.md` (Prefer-random-over-idioms), the right hook is to
  weight `+0.0` and `-0.0` higher in the random f32 constant pool and
  ensure `fsub C, x` patterns are reachable -- the bad fold will then
  surface on any kernel that pairs a `fsub +0,x` with a VOP3 user.
