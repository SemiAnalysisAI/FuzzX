# 207 — SimplifyLibCalls `optimizeFdim` constant-folds `fdim(±Inf, ±Inf)` to qNaN instead of +0

Component: `llvm/lib/Transforms/Utils/SimplifyLibCalls.cpp` lines ~3179-3203

`optimizeFdim` evaluates `fdim(X,Y)` as `max(X - Y, +0)` (line 3197-3201). But C99 §7.12.12.1 defines `fdim(x,y)` as `x − y` *if `x > y`*, **`+0` otherwise**. For `x == y == +Inf` (or `−Inf`), `x > y` is false so the answer is `+0`. Doing the subtraction first produces `Inf − Inf = qNaN`, and `maximum(qNaN, +0)` propagates the NaN per IEEE 754-2019 `maximum`.

glibc and musl return `+0` for both cases; LLVM's fold returns qNaN. The fold replaces the libcall with a literal qNaN IR constant — there's no errno/observable-side-effect escape hatch since the callee is declared `memory(none)`.

## Reproducer

`opt -passes=instcombine -S repro.ll`:
- `test_inf_inf()` → `ret double +qnan` (expected: `ret double +0.0`)
- `test_ninf_ninf()` → `ret double +qnan` (expected: `ret double +0.0`)

## Severity

Real Alive2-falsifiable miscompile in default `-O2` (`optimizeFdim` runs in InstCombine for libc `fdim`).

## Fix

Compare first: if `X.isFinite() && Y.isFinite()` use `max(X-Y, 0)`; otherwise return `X > Y ? X-Y : +0`, mirroring the C99 wording.
