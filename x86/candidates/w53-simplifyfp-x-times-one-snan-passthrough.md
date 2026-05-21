# w53: `simplifyFPBinop` / `simplifyFDivInst` reduce `X*1.0` and `X/1.0` to `X`, passing sNaN through unchanged

**File:lines:**
- `llvm/lib/CodeGen/SelectionDAG/SelectionDAG.cpp:11615-11619` (`simplifyFPBinop` — `X * 1.0 -> X`, `X / 1.0 -> X`)
- `llvm/lib/Analysis/InstructionSimplify.cpp:6068-6070` (`simplifyFMAFMul` — `X * 1.0 -> X`)
- `llvm/lib/Analysis/InstructionSimplify.cpp:6164-6166` (`simplifyFDivInst` — `X / 1.0 -> X`)

## Reasoning

Three sister identities, all unguarded by `nnan`:

```cpp
// SelectionDAG.cpp simplifyFPBinop
if (Opcode == ISD::FMUL || Opcode == ISD::FDIV)
  if (YC->getValueAPF().isExactlyValue(1.0))
    return X;
```

```cpp
// InstructionSimplify.cpp simplifyFMAFMul
// X * 1.0 --> X
if (match(Op1, m_FPOne()))
  return Op0;
```

```cpp
// InstructionSimplify.cpp simplifyFDivInst
// X / 1.0 -> X
if (match(Op1, m_FPOne()))
  return Op0;
```

LangRef permits non-strict FP ops to return an unspecified NaN payload for NaN
operands, so technically these folds are *spec-permitted*. But the
`constrained.fdiv/fmul` strict counterparts (lines 6109-6111, 6154-6166) DO
emit a real `mulsd`/`divsd`, which quiets sNaN. The result is a **strict↔
non-strict observable-bit cliff**: the user's choice of intrinsic invisibly
controls whether sNaN bits leak through.

## Confirmed wrong runtime bits on x86_64

IR (`/tmp/w53_fmul1.ll`):

```ll
define double @fmul1(double %x) { %r = fmul double %x, 1.0  ret double %r }
define double @fdiv1(double %x) { %r = fdiv double %x, 1.0  ret double %r }
```

`llc -mtriple=x86_64-linux-gnu` produces just `retq` for both functions.

Driver:

```c
uint64_t snan_bits = 0x7FF4000000000000ULL;  // sNaN, mantissa MSB=0
double x; __builtin_memcpy(&x, &snan_bits, 8);
double r1 = fmul1(x);  uint64_t b1; __builtin_memcpy(&b1, &r1, 8);
double r2 = fdiv1(x);  uint64_t b2; __builtin_memcpy(&b2, &r2, 8);
```

Observed:
```
input bits  = 0x7ff4000000000000  (sNaN)
fmul x,1.0  = 0x7ff4000000000000  (unchanged sNaN -- bit 51 still 0)
fdiv x,1.0  = 0x7ff4000000000000  (unchanged sNaN -- bit 51 still 0)
```

The strict variants of the same IR emit `mulsd`/`divsd` (verified) and produce
`0x7ffc000000000000` (qNaN with bit 51 set), as a real CPU FP op must.

## Why this is interesting even though LangRef permits it

1. **Strict-vs-non-strict ABI cliff.** A user converting a function from
   `constrained.fmul` to plain `fmul` (typically for performance) silently
   acquires a new observable bit-pattern semantic. This is the most direct,
   universally-reproducible case.
2. **The fold applies to vector ops too.** `<2 x double> X * splat(1.0)` and
   `<4 x float> X * splat(1.0)` both hit `simplifyFPBinop` and lose sNaN
   quieting on every lane.
3. **Composable with sign-bit folds.** Worker w12 / w54 / this candidate w53
   chain demonstrate that NaN-payload-preserving identities span at least
   FMUL/FDIV/FSUB/FADD identity simplifications, plus visitFMUL's
   `(fmul X,-1.0) -> (fsub -0.0,X)` and Reassociate's
   `LowerNegateToMultiply`. NaN-bit-aware lowering is essentially absent in
   non-strict mode.

## Note on classification

Filed as a "near-miscompile" / observable-bit-pattern bug rather than a hard
miscompile, since LangRef explicitly allows the fold. Real-world impact:
JS-style NaN-boxing, GPU shading languages, distributed system FP
reproducibility hooks, and code that does `if (isSNaN(x))` after a
load/store + arithmetic op. Worth tracking because it's the *simplest*
non-strict-FP behavior to test for and is structurally distinct from the
visitFSUB FIXME w12 already covers.
