# 248 — InstCombine `foldSelectIntoOp` wrongly stamps `ninf` on the new select via `TVI->hasNoInfs()`

Component: `llvm/lib/Transforms/InstCombine/InstCombineSelect.cpp` lines ~631-634

When folding `select c, binop(x, y), x` → `binop(x, select(c, y, identity))`, the new select's FMF logic is:
```cpp
NewSelFMF.setNoInfs(TVI->hasNoInfs() ||
                    (CanInferFiniteOperandsFromResult &&
                     NewSelFMF.noInfs() && NewSelFMF.noNaNs()));
```

The first disjunct `TVI->hasNoInfs()` is wrong on its own — `ninf` on a binop is a *result-only* property and does not imply that the *operands* are finite. The in-source comment two lines above (619-626) acknowledges this for `fdiv`, but the implementation propagates `ninf` from the binop unconditionally.

For `fmul ninf(0, +inf)` the binop's RESULT is `NaN` (satisfies `ninf`: NaN is not inf), but transferring `ninf` to a new select that returns `+inf` directly poisons the IR.

## Reproducer

```ll
%x = sitofp i32 %i to double
%m = fmul ninf double %x, %y
%r = select i1 %c, double %m, double %x
```

`opt -passes=instcombine -S` →
```
%m = select ninf i1 %c, double %y, double 1.000000e+00
%r = fmul double %m, %x
```

With `%i = 0, %y = +inf, %c = true`:
- Original: `fmul ninf(0.0, +inf)` = `NaN` (satisfies `ninf` because NaN ≠ inf), `select true, NaN, 0.0` = `NaN`. **Defined NaN value.**
- Optimized: `select ninf true, +inf, 1.0` = `+inf`, which violates `ninf` → **poison**. `fmul poison, 0.0` → poison.

Real Alive2-falsifiable miscompile.

## Severity

Default x86 -O2. Affects any code that uses `fmul/fdiv ninf` to indicate "result is finite" — InstCombine misinterprets it as "operands are finite".

## Fix

Drop the standalone `TVI->hasNoInfs()` arm; rely only on `CanInferFiniteOperandsFromResult && select.ninf && select.nnan`. Or prove `OOp` is finite via `computeKnownFPClass(OOp, ..., fcInf).isKnownNeverInfinity()` before propagating `ninf`.

---

## CORRECTION (re-audit at HEAD) — NOT A BUG

The premise that `fmul ninf 0.0, +inf` is a "defined NaN" is wrong under the
current LangRef definition of `ninf`:

> No Infs — Allow optimizations to assume the arguments and result are not
> +/-Inf. **If an argument is +/-Inf**, or the result would be +/-Inf, it
> produces a poison value instead.

So `fmul ninf 0.0, +inf` already produces **poison** (an argument is +Inf), and
`select c, poison, x` is poison whenever that path is taken. Whenever the source
select is *defined*, the folded operand `OOp` is non-Inf, so the `ninf`-stamped
new select cannot return an Inf and cannot introduce poison the source lacked.
The standalone `TVI->hasNoInfs()` disjunct is therefore sound; the in-source
comment ("the flag only guarantees the result is never infinity") reflects an
older, result-only reading of `ninf`. **WONTFIX.**
