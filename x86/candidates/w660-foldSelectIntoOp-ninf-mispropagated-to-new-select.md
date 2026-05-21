# w660: foldSelectIntoOp propagates `ninf` to new select unconditionally from TVI

## Severity
Miscompile (creates poison where original IR was a defined value).

## File / lines
`llvm/lib/Transforms/InstCombine/InstCombineSelect.cpp`
- The buggy condition: lines **631-634**
- Full transform: `InstCombinerImpl::foldSelectIntoOp` (lines 569-658) via lambda
  `TryFoldSelectIntoOp` (lines 573-649).

```cpp
NewSelFMF.setNoInfs(TVI->hasNoInfs() ||
                    (CanInferFiniteOperandsFromResult &&
                     NewSelFMF.noInfs() && NewSelFMF.noNaNs()));
cast<Instruction>(NewSel)->setFastMathFlags(NewSelFMF);
```

`CanInferFiniteOperandsFromResult` is true for FAdd/FSub/FMul only
(lines 627-630). FDiv is intentionally excluded per the comment at
lines 619-626 ("does not hold for fdiv"). However the disjunction
includes `TVI->hasNoInfs()` as an **independent** first branch — so
whenever `TVI` (the binop branch of the original select) carries
`ninf`, we slap `ninf` on the *new* select regardless of binop opcode.

That is wrong. `binop ninf X, OOp` says "the **binop result** is not
inf" – it does **not** say "OOp is not inf". The new select picks
between OOp and the identity constant. If OOp can be `+/-inf`, the
new select can evaluate to `+/-inf`, which under `ninf` is poison.

## Cases where the bug fires

1. **FDiv** (Mistakenly relies on `TVI->hasNoInfs()`): e.g. `fdiv ninf
   X, OOp` where `X = 0.0` is finite and `OOp = +inf` →
   `0.0 / +inf = +0.0`, `ninf` satisfied. After fold the new select
   `select ninf c, +inf, 1.0` is poison when `c == true`.

2. **FMul** (Mistakenly relies on `TVI->hasNoInfs()` alone, without
   requiring `nnan` on the select): e.g. `fmul ninf X, OOp` where
   `X = 0.0`, `OOp = +inf` → `0.0 * +inf = NaN`, which satisfies `ninf`
   (NaN is not inf). After fold the new select
   `select ninf c, +inf, 1.0` is poison.

## Repro (`opt -passes=instcombine -S`)

`/tmp/sel_bugs/w660-fmul-confirm.ll`:

```llvm
define float @bug(i1 %c, i32 %i, float %y) {
  %x = sitofp i32 %i to float          ; x is finite (never NaN/inf)
  %m = fmul ninf float %x, %y          ; result not inf; says nothing about y
  %r = select i1 %c, float %m, float %x
  ret float %r
}
```

### opt output (`opt -passes=instcombine -S`)
```
define float @bug(i1 %c, i32 %i, float %y) {
  %x = sitofp i32 %i to float
  %m = select ninf i1 %c, float %y, float 1.000000e+00   ; <-- ninf NOT justified
  %r = fmul float %m, %x
  ret float %r
}
```

### Diff (concrete inputs: `c=true, i=0, y=+inf`)

| Step | Original IR | After instcombine |
| --- | --- | --- |
| `x` | `0.0` | `0.0` |
| middle | `%m = fmul ninf 0.0, +inf = NaN` (ninf satisfied: NaN is not inf) | `%m = select ninf true, +inf, 1.0` → **POISON** (ninf says not inf, but value is +inf) |
| return | `select true, NaN, 0.0 = NaN` | `fmul POISON, 0.0 = POISON` |

Original returns `NaN`; new returns `poison`. A consumer that uses the
return value (e.g. `fcmp uno %r, %r` to check NaN, or any non-poison
guard via `freeze`) miscompiles.

### Second repro (FDiv path):
`/tmp/sel_bugs/w660-repro1.ll`:

```llvm
define float @bug(i1 %c, i32 %i, float %y) {
  %x = sitofp i32 %i to float
  %d = fdiv ninf float %x, %y
  %r = select i1 %c, float %d, float %x
  ret float %r
}
```

opt output:
```
define float @bug(i1 %c, i32 %i, float %y) {
  %x = sitofp i32 %i to float
  %d = select ninf i1 %c, float %y, float 1.000000e+00
  %r = fdiv float %x, %d
  ret float %r
}
```

`c=true, i=0, y=+inf`: original `fdiv ninf 0.0, +inf = +0.0` (ninf
holds), select returns `+0.0`. New: select ninf evaluates `+inf` →
poison, fdiv `0.0, poison = poison`. Original `+0.0`, new `poison` →
miscompile.

## Root cause

The intent encoded in the comment (lines 619-626) is correct: for
fadd/fsub/fmul, **if the select itself carries `ninf` AND `nnan`**, the
operands are forced finite, so it's safe to mark NewSel `ninf`. The
implementation, however, takes `TVI->hasNoInfs()` as a sufficient
condition by itself. That's never sufficient, because `ninf` on the
binop is a *result-only* property.

## Suggested fix sketch

Drop the standalone `TVI->hasNoInfs()` disjunct, so `ninf` only
propagates when the *select* result is known finite, which is what the
preceding lines (and the comment) were trying to express:

```cpp
NewSelFMF.setNoInfs(CanInferFiniteOperandsFromResult &&
                    NewSelFMF.noInfs() && NewSelFMF.noNaNs());
```

(Or, if a separate `TVI`-based path is desired, it must additionally
prove that the non-`FalseVal` operand is finite — e.g. via
`computeKnownFPClass(OOp, ..., fcInf).isKnownNeverInfinity()`.)

## Discovery
Reading the file with attention to the FMF intersection logic
introduced by the May 2026 refactor (see `git blame` lines 631-634).
The comment at 619-626 explicitly flags FDiv as not having the
"finite-result implies finite-operands" property, which made the OR
with `TVI->hasNoInfs()` look suspect; a 3-line LL test confirmed.
