# m135: `AMDGPULibCalls::fold_rootn` rewrites `rootn(x, ±2)` to `sqrt`/`rsqrt` without FMF gate, losing OpenCL sign-of-zero and negative-base rules

*Discovery method: code inspection.*  Sibling shape to m093
(`pow(x, ±0.5)` -> sqrt without fmf) and m130 (`powr(x<0, c)` ignores
spec).  Unlike m093, this fold uses `Intrinsic::sqrt` directly and
does NOT need a module-visible `_Z4sqrtf` body to fire.

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/AMDGPULibCalls.cpp:1171-1189`
(`rootn(x, 2) -> sqrt(x)`) and `:1209-1235` (`rootn(x, -2) -> rsqrt(x)`).

Both folds are gated only by `shouldReplaceLibcallWithIntrinsic`
(which checks type / strictfp / minsize -- NO FMF).  They fire
whenever `_Z5rootnfi` is declared (the common OpenCL/HIP case).

Per OpenCL 3.0 builtins for `rootn(x, n)` with even `n`:

| input | OpenCL `rootn` | LLVM `sqrt`/`rsqrt` | mismatch |
| --- | --- | --- | --- |
| `rootn(-0.0, 2)`   | `+0.0` (sign dropped, even root) | `sqrt(-0.0)` = `-0.0`              | yes |
| `rootn(-Inf, 2)`   | NaN (negative base, even root)    | `sqrt(-Inf)` = -qNaN (`0xFFC00000`) | sign mismatch |
| `rootn(-0.0, -2)`  | `+Inf` (n<0 even)                 | `1/sqrt(-0.0)` = `1/-0.0` = `-Inf`  | sign mismatch |
| `rootn(-Inf, -2)`  | `+0.0` (n<0 even)                 | `1/sqrt(-Inf)` = NaN                | value+kind mismatch |

## Reproducer

`reduced.ll`:

```llvm
define amdgpu_kernel void @t(ptr addrspace(1) %in, ptr addrspace(1) %out) {
  %x = load float, ptr addrspace(1) %in
  %r = tail call float @_Z5rootnfi(float %x, i32 2)
  store float %r, ptr addrspace(1) %out
  ret void
}
```

`run_ll_reproducer.sh` output:

```
[0] input=0x80000000 O0=0x00000000 O2=0x80000000 mismatch=true   ; rootn(-0,2)
[1] input=0xff800000 O0=0x7fc00000 O2=0xffc00000 mismatch=true   ; rootn(-Inf,2)
any_mismatch=true
```

The harness ships an in-module OCL-correct `_Z5rootnfi` body for the
corner cases at -O0 (which is preserved), while at -O2 the
`AMDGPULibCalls::fold_rootn` rewrite replaces the call with
`Intrinsic::sqrt` directly, losing the OCL-specific sign handling.

## Suggested fix

Gate both folds on `FPOp->hasNoSignedZeros() && FPOp->hasNoInfs()`,
or special-case the `+0.0` / `-Inf` arms by emitting an explicit
select:

```cpp
if (Y == 2) {
  if (!FPOp->hasNoSignedZeros() || !FPOp->hasNoInfs())
    return false;
  ...
}
```

Same fix for the `Y == -2` arm.  The `-1` arm (already noted in
post-m093 audit) needs the same fix.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Reproduces (`O0=0x00000000, O2=0x80000000`). |
| ROCm 7.1.1 (`opt -passes=amdgpu-simplifylib`) | Same fold, same defect. |

## Why the fuzzer hasn't caught it

* The IR fuzzer emits intrinsics, not OpenCL-mangled libcalls
  (`_Z5rootnfi`).  Per `MEMORY.md` (Prefer-random-over-idioms), the
  right hook is to inject `_Z5rootnfi`/`_Z4powrff`/`_Z3powff`
  declarations + uses into the random emitter.
* The differential oracle compares against IR semantics of
  `llvm.sqrt`/`llvm.rsqrt`, which match the buggy fold -- needs an
  OCL-aware oracle to surface the divergence.
