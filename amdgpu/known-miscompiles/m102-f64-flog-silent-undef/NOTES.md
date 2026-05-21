# m102: f64 `FLOG / FLOG2 / FLOG10` silently miscompile -- llc emits zero/undef store instead of the log call

*Discovery method: code inspection.*

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/AMDGPUISelLowering.cpp:419-427`:

```cpp
setOperationAction(ISD::FLOG2, MVT::f32, Custom);
...
setOperationAction(
    {ISD::FLOG, ISD::FLOG10, ISD::FEXP, ISD::FEXP2, ISD::FEXP10}, MVT::f32,
    Custom);
setOperationAction({ISD::FEXP, ISD::FEXP2, ISD::FEXP10}, MVT::f64, Custom);
```

The EXP family is marked Custom for f64 (and `lowerFEXPF64` at
`:2942` implements a real polynomial), but the LOG family is **not**.
`ISD::FLOG`, `ISD::FLOG2`, `ISD::FLOG10` on f64 therefore fall through
generic Expand, which tries a libcall that AMDGPU does not provide.

The corresponding `LowerFLOG2` (`:2748`) and `LowerFLOGCommon`
(`:2789`) helpers are never invoked with f64 -- and would not work
even if they were: they emit `AMDGPUISD::LOG` in the result VT, but
`VOP1Instructions.td:354-355` only defines `V_LOG_F32`.  There is no
ISA primitive for an f64 log to lower onto.

## Symptoms

Non-strict (default IR):

```
$ llc -mtriple=amdgcn -mcpu=gfx950 -O2 log2_f64.ll
error: no libcall available for flog2
```

llc exits `0`, and the emitted kernel has had the log call **silently
dropped**:

```asm
k:
        s_load_dwordx4 s[0:3], s[4:5], 0x24
        v_mov_b32_e32 v0, 0
        s_waitcnt lgkmcnt(0)
        global_store_dwordx2 v0, v[0:1], s[0:1]   ; stores 0, undef
        s_endpgm
```

The user wrote `llvm.log2.f64`; the compiler emitted "store
{0, undef}".  Caller observes uninitialized output.

Strict (`strictfp`): hard crash.

```
LLVM ERROR: unsupported library call operation
[abort in TargetLowering::makeLibCall during SelectionDAG::Legalize]
```

Same behaviour for `llvm.log.f64`, `llvm.log10.f64`, with and
without the `afn` fast-math flag, on gfx900, gfx950, gfx1100 alike
(target-agnostic legality table).

## How the buggy shape arises

Trivial source IR -- any f64 use of `llvm.log*`:

```c
double k(double x) { return __builtin_log2(x); }
```

`clang -O2 -target amdgcn-amd-amdhsa -mcpu=gfx950`.

The IR-level intrinsic survives untouched until SDAG legalization,
where the missing Custom registration sends it to the no-op libcall
path.

## Reproducer

`reduced.ll`:

```llvm
declare double @llvm.log2.f64(double)
define amdgpu_kernel void @k(ptr addrspace(1) %out, double %x) {
  %r = call double @llvm.log2.f64(double %x)
  store double %r, ptr addrspace(1) %out
  ret void
}
```

Input: `x = 2.0` (`0x4000000000000000`).  Expected output:
`log2(2.0) = 1.0` (`0x3FF0000000000000`).  Observed output: `0, undef`.

The full kernel form in `reduced.ll` packs that into the standard
RUN-INPUTS harness shape (i32 word inputs that get bitcast to f64).

## Suggested fix

The right fix is to make the f64 log family Custom and provide an
`lowerFLOGF64` polynomial that mirrors `lowerFEXPF64`:

```cpp
setOperationAction({ISD::FLOG, ISD::FLOG2, ISD::FLOG10}, MVT::f64, Custom);
```

The polynomial can do range reduction (`frexp` to mantissa + exponent),
evaluate `log2(mantissa)` via a degree-N minimax polynomial in f64
FMA chains (the ISA has `v_fma_f64`), then add the integer exponent.
Same architecture as `lowerFEXPF64`.

An interim mitigation that at least surfaces the error: register a
libcall name in `AMDGPUAS::LibCallList` so `makeLibCall` emits a real
call (the user would then get a link error instead of a silent
miscompile).

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Silent miscompile: llc prints error, exits 0, emits store of `{0, undef}`. |
| ROCm 7.1.1 (`/opt/rocm-7.1.1/lib/llvm/bin/clang-20`) | Same -- `setOperationAction` for f64 FLOG family also absent. |

## Why the fuzzer doesn't see it

* The current AMDGPU IR fuzzer's FP intrinsic emit does not include
  the f64 `llvm.log*` family.
* The interpreter oracle skips kernels with `llvm.log.f64` because the
  current FuzzX harness compiles the same IR with O0 and O2 from the
  same backend; both produce the same `{0, undef}` so the differential
  check sees `any_mismatch=false`.  A correct reference compile is
  needed to flag this -- or simply marking llvm.log.f64 calls as
  "expected to error" in the static SDAG legality table.
