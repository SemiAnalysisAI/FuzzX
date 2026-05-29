# 115 — `SelectionDAG::simplifyFPBinop` identity folds bypass sNaN quieting (fmul/fdiv X, 1.0)

Component: `llvm/lib/CodeGen/SelectionDAG/SelectionDAG.cpp:11584-11627`
(`SelectionDAG::simplifyFPBinop`).

`fmul X, 1.0` and `fdiv X, 1.0` are folded to `X` without any `nnan` flag check.
On x86, both functions lower to nothing but `retq` — `%xmm0` is returned
verbatim. For a sNaN input (e.g. `0x7FA00000`), the lowered code returns the
raw sNaN; the hardware `mulss`/`divss` against the constant `1.0` would have
quieted to `0x7FE00000` and raised the invalid-operation flag. The fold
silently discards both the bit-pattern change and the FP exception.

LangRef does state NaN bit patterns are unspecified, but several real-world
programs and frontends rely on `x = x * 1.0f;` or `x = x / 1.0f;` as a
canonical quieting idiom; the fold turns them into a no-op.

See also `bugs/112-fp-round-of-fp-extend-elides-snan-quieting/` for the
analogous fpext/fptrunc round-trip fold.

## Reproduction
```
$ ./cmd.sh
fmul_one:    retq
fdiv_one:    retq
```
