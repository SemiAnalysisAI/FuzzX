# m123: `lowerFastUnsafeFDIV64` NR chain returns NaN for runtime zero divisor under `afn` (IEEE says +/-Inf)

*Discovery method: code inspection.*  Sibling of m075/m077/m104/m122
(special-value-blind FP lowering at the SDAG f64 layer).

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/SIISelLowering.cpp:13140-13178`
(`lowerFastUnsafeFDIV64`):

`fdiv afn double X, Y` with runtime `Y == 0.0` lowers to:

```
R    = RCP(Y)           ; v_rcp_f64
Tmp0 = FMA(-Y, R, 1.0)  ; v_fma_f64
R    = FMA(Tmp0, R, R)
Tmp1 = FMA(-Y, R, 1.0)
R    = FMA(Tmp1, R, R)
Mul  = FMUL(X, R)        ; (or analogous for -1.0/Y)
```

For runtime `Y = 0.0`:
1. `RCP(0)` returns `+/-Inf` (`v_rcp_f64` HW result).
2. `FMA(-0, +Inf, 1.0)` is `(-0 * +Inf) + 1.0` = `NaN + 1.0` = `NaN`.
3. All subsequent FMAs propagate NaN; the final `FMUL(X, NaN)` is
   `NaN`.

But IEEE / AMDGCN-RCP say `X / +0 = sign(X) * Inf`.

LangRef `afn` ("approximate functions") permits *approximate*
substitutions; it does NOT permit silently turning `+Inf` into `NaN`.
That would require `ninf` (no Infs assumed) + `nnan` (no NaNs
assumed) together.

The f32 fast path at `SIISelLowering.cpp:13136-13137` uses a simple
`X * RCP(Y)` and is safe (`X * +Inf = +/-Inf` per HW v_mul_f32).
Only the afn-f64 path is affected, including the `-1.0/Y` and
`1.0/Y` sub-paths at lines 13153-13174.

## Reproducer

`reduced.ll`:

```llvm
define amdgpu_kernel void @t(ptr addrspace(1) %out, double %y) {
  %r = fdiv afn double 1.0, %y
  store double %r, ptr addrspace(1) %out
  ret void
}
```

Codegen with `clang -mcpu=gfx950 -O2`:

```asm
v_rcp_f64_e32  v[0:1], s[2:3]
v_fma_f64      v[2:3], -s[2:3], v[0:1], 1.0    ; Tmp0
v_fmac_f64_e32 v[0:1], v[2:3], v[0:1]
v_fma_f64      v[2:3], -s[2:3], v[0:1], 1.0
v_fmac_f64_e32 v[0:1], v[2:3], v[0:1]
```

For runtime `y = 0.0`:

* Expected (IEEE / non-afn path with `DIV_FIXUP`): `+Inf =
  0x7FF0000000000000`.
* Observed: `NaN = 0x7FF8000000000000` (qNaN propagated through the
  NR chain).

All-constant `fdiv afn double 1.0, 0.0` is folded earlier and doesn't
trigger -- needs a runtime zero divisor.

## Why no runtime O0/O2 mismatch in the FuzzX harness

The lowering is Custom legalization that runs at all -O levels; both
-O0 and -O2 emit the same buggy NR chain.  The witness is SDAG
vs IR semantics (or `afn` vs non-`afn` cross-check) -- not captured
by the O0-vs-O2 oracle.

## Suggested fix

Two options:

```cpp
// (a) Refuse the optimization unless both ninf and nnan are set:
if (!Flags.hasNoInfs() || !Flags.hasNoNaNs())
  return SDValue();
```

```cpp
// (b) Add a guard at the end of the NR chain:
SDValue IsInf = DAG.getNode(ISD::FCMP, SL, MVT::i1, R, +Inf, OEQ);
SDValue Result = DAG.getSelect(SL, MVT::f64, IsInf, +Inf, R);
```

Option (a) is simpler and aligns with the f32 fast path's stricter
preconditions.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Reproduces (NR chain emitted, no IsInf guard). |
| ROCm 7.1.1 (`/opt/rocm-7.1.1/lib/llvm/bin/clang-20`) | Same lowering. |

## Why the fuzzer hasn't caught it

* The FP emitter rarely seeds `0.0` as the divisor of a `fdiv afn
  double`.
* The interpreter oracle currently skips f64 fdiv with non-finite
  outputs.
* Per `MEMORY.md` (Prefer-random-over-idioms), the right hook is to
  weight `0.0`/`-0.0`/`+Inf`/`-Inf` higher in the f64 constant pool
  and emit `fdiv afn` with mixed runtime/constant operands so this
  shape surfaces.
