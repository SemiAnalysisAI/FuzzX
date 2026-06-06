# Backend-independent LLVM bug hunt

*Human-written portion*

These entries track LLVM IR optimizer bugs that are not tied to a backend. The
initial batch focuses on InstCombine and the InstructionSimplify folds reached
by `opt`.

The interesting cases here are miscompiles, assertion failures, and crashes.
Missed optimizations and metadata-only losses are intentionally out of scope.

The main catalog below is restricted to bugs that manifest on ordinary targets
with integer pointer types. Fastmath/libm issues and non-integral pointer
representation issues are kept in separate categories so they do not inflate the
ordinary-target count.

Everything below here is machine-generated. Good luck.

------------

Goal: find at least 20 InstCombine/InstructionSimplify bugs with unique root
causes that matter on ordinary integer-pointer targets.

**Status: 20 live ordinary-target unique roots. 22 live FP/libm rows are kept
separately. 18 historical non-integral pointer representation candidates are
kept separately and deprioritized.**

Strict constrained-FP/value-FMF gray cases are not listed here. For pointer
semantics, the relevant LangRef distinction is: `ptrtoint` observes the full
pointer representation, while `ptrtoaddr` and pointer `icmp` are address/index
comparisons. Bugs that only depend on non-address bits, external pointer state,
or non-integral pointer representation are not counted in the ordinary-target
total.

## Tools

- LLVM source: `/Users/justinlebar/code/llvm2/llvm`
- opt: `/Users/justinlebar/code/llvm2/build/bin/opt`
- LLVM checkout used while confirming: `main` at `fd68fe73c8be`

## Layout

- `bugs/NNN-short-name/` - one folder per live confirmed bug
- `repro.ll` - minimal IR reproducer
- `cmd.sh` - exact command to reproduce the current optimizer behavior
- `NOTES.md` - explanation, source pointer, witness, and verifier agent

## Ordinary Integer-Pointer / Integer-Vector Catalog

These 20 entries are the in-scope set for the current goal: ordinary integer or
integer-pointer IR, no fastmath/strictfp dependency, and no non-integral pointer
representation dependency. Entries marked "fixpoint abort" are assertion-enabled
InstCombine failures where the pass changes the IR on a later verification
iteration.

| # | Bug | Root cause / status |
|---|-----|---------------------|
| 001 | [001-instcombine-store-poison-fixpoint-abort](bugs/001-instcombine-store-poison-fixpoint-abort/) | `store poison` deletion exposes another fold after the fixpoint check; assertion failure. Confirmed by Harvey. |
| 002 | [002-instcombine-log2ceil-i2-nsw-poison](bugs/002-instcombine-log2ceil-i2-nsw-poison/) | log2-ceil idiom emits `sub nsw` that poisons a defined `i2` result. Confirmed by Kant. |
| 024 | [024-instcombine-store-undef-overwrites-poison](bugs/024-instcombine-store-undef-overwrites-poison/) | deleting `store undef` can expose an earlier poison store. Confirmed by Kuhn. |
| 025 | [025-instcombine-wide-load-compare-fixpoint-abort](bugs/025-instcombine-wide-load-compare-fixpoint-abort/) | wide load/compare fold exposes duplicate-store DSE only on a later iteration; fixpoint abort. Confirmed by Confucius. |
| 026 | [026-instcombine-partial-aggregate-store-lane-poison](bugs/026-instcombine-partial-aggregate-store-lane-poison/) | aggregate-to-vector store rewrite propagates poison into lanes that were `undef`. Confirmed by Zeno. |
| 027 | [027-instcombine-alloca-load-duplicate-store-fixpoint-abort](bugs/027-instcombine-alloca-load-duplicate-store-fixpoint-abort/) | alloca load forwarding exposes duplicate-store DSE after the fixpoint check. Confirmed by Peirce. |
| 028 | [028-instcombine-global-initializer-fixpoint-abort](bugs/028-instcombine-global-initializer-fixpoint-abort/) | global initializer forwarding exposes a second InstCombine iteration. Confirmed by Ohm. |
| 029 | [029-instcombine-sink-domconditioncache-fixpoint-abort](bugs/029-instcombine-sink-domconditioncache-fixpoint-abort/) | sunk instructions are revisited before `DomConditionCache` has the needed condition facts. Confirmed by Hilbert. |
| 030 | [030-instcombine-loop-flag-store-forward-xor-fixpoint-abort](bugs/030-instcombine-loop-flag-store-forward-xor-fixpoint-abort/) | loop flag store forwarding exposes `X \| ~X` only on a later iteration. Confirmed by Boyle. |
| 031 | [031-instcombine-assume-knownbits-fixpoint-abort](bugs/031-instcombine-assume-knownbits-fixpoint-abort/) | same-block `assume` changes known-bits reasoning after an earlier consumer ran. Confirmed by Socrates. |
| 032 | [032-instcombine-select-to-and-fixpoint-abort](bugs/032-instcombine-select-to-and-fixpoint-abort/) | logical-select canonicalization only relaxes to bitwise `and` on a later iteration. Confirmed by Hypatia. |
| 033 | [033-instcombine-load-cse-scanlimit-fixpoint-abort](bugs/033-instcombine-load-cse-scanlimit-fixpoint-abort/) | bounded available-load scan misses CSE until the block is shortened. Confirmed by Helmholtz. |
| 034 | [034-instcombine-icmp-or-xor-oneuse-fixpoint-abort](bugs/034-instcombine-icmp-or-xor-oneuse-fixpoint-abort/) | one-use precondition changes after a sibling compare folds. Confirmed by Sagan. |
| 035 | [035-instcombine-indexed-gep-compare-nuw-fixpoint-abort](bugs/035-instcombine-indexed-gep-compare-nuw-fixpoint-abort/) | indexed GEP compare rewrite exposes later `nuw` inference. Confirmed by Ptolemy. |
| 036 | [036-instcombine-assume-redundant-iv-gep-nuw-fixpoint-abort](bugs/036-instcombine-assume-redundant-iv-gep-nuw-fixpoint-abort/) | IV simplification exposes redundant-assume GEP `nuw` inference. Confirmed by Singer. |
| 037 | [037-instsimplify-noalias-allocation-multi-compare](bugs/037-instsimplify-noalias-allocation-multi-compare/) | non-escaping allocation comparisons are folded independently even though the choices do not compose. Confirmed by Maxwell. |
| 038 | [038-instcombine-scalarizephi-invoke-incoming-crash](bugs/038-instcombine-scalarizephi-invoke-incoming-crash/) | `scalarizePHI` inserts an `extractelement` after an `invoke` terminator. Confirmed by Kant the 2nd. |
| 039 | [039-instcombine-phi-samesign-nonzero-canonicalization](bugs/039-instcombine-phi-samesign-nonzero-canonicalization/) | PHI nonzero canonicalization changes signs that matter to `icmp samesign`. Confirmed by Anscombe the 2nd. |
| 041 | [041-instsimplify-active-lane-mask-wrap](bugs/041-instsimplify-active-lane-mask-wrap/) | `llvm.get.active.lane.mask` constant fold wraps arithmetic that LangRef defines over mathematical integers. Confirmed by Franklin the 2nd. |
| 042 | [042-instcombine-fshl-zero-shift-lane-poison](bugs/042-instcombine-fshl-zero-shift-lane-poison/) | `fshl(0, X, C)` becomes `lshr X, (BW-C)`, creating lane poison when `C == 0`. Confirmed by Turing the 2nd. |

## FP / Libm / Fastmath Catalog

These are live reproducers, but they are not counted in the ordinary-target
unique-root total. Rows 003-023 are mostly siblings of one broad issue: a
multi-instruction or libm rewrite uses rewrite permission from only the outer
instruction/call even though the participating inner operations do not carry the
needed fast-math flags. Row 040 is a separate FP intrinsic bug with no FMF or
strictfp requirement.

| # | Bug | Root cause / status |
|---|-----|---------------------|
| 003 | [003-instcombine-fdiv-constant-fmul-needs-arcp](bugs/003-instcombine-fdiv-constant-fmul-needs-arcp/) | `(x / C) * K` becomes `x * (K/C)` with only `reassoc`; needs `arcp`. |
| 004 | [004-instcombine-tan-cos-to-sin-without-afn](bugs/004-instcombine-tan-cos-to-sin-without-afn/) | `tan(x) * cos(x)` becomes `sin(x)` without `afn`. |
| 005 | [005-instcombine-pow-times-x-unflagged-pow](bugs/005-instcombine-pow-times-x-unflagged-pow/) | `pow(x, y) * x` is folded through an unflagged `pow`. |
| 006 | [006-instcombine-pow-products-same-base-unflagged](bugs/006-instcombine-pow-products-same-base-unflagged/) | `pow(x, y) * pow(x, z)` is folded through unflagged `pow` calls. |
| 007 | [007-instcombine-pow-products-same-exponent-unflagged](bugs/007-instcombine-pow-products-same-exponent-unflagged/) | `pow(x, y) * pow(z, y)` is folded through unflagged `pow` calls. |
| 008 | [008-instcombine-exp-product-unflagged](bugs/008-instcombine-exp-product-unflagged/) | `exp(x) * exp(y)` is folded through unflagged `exp` calls. |
| 009 | [009-instcombine-exp2-product-unflagged](bugs/009-instcombine-exp2-product-unflagged/) | `exp2(x) * exp2(y)` is folded through unflagged `exp2` calls. |
| 010 | [010-instcombine-sin-div-cos-without-afn](bugs/010-instcombine-sin-div-cos-without-afn/) | `sin(x) / cos(x)` becomes `tan(x)` without `afn`. |
| 011 | [011-instcombine-cos-div-sin-without-afn](bugs/011-instcombine-cos-div-sin-without-afn/) | `cos(x) / sin(x)` becomes `1.0 / tan(x)` without `afn`. |
| 012 | [012-instcombine-pow-div-x-unflagged-pow](bugs/012-instcombine-pow-div-x-unflagged-pow/) | `pow(x, y) / x` is folded through an unflagged `pow`. |
| 013 | [013-instsimplify-exp-log-outer-reassoc](bugs/013-instsimplify-exp-log-outer-reassoc/) | `exp(log(x))` folds with only outer-call `reassoc`. |
| 014 | [014-instsimplify-exp2-log2-outer-reassoc](bugs/014-instsimplify-exp2-log2-outer-reassoc/) | `exp2(log2(x))` folds with only outer-call `reassoc`. |
| 015 | [015-instsimplify-exp10-log10-outer-reassoc](bugs/015-instsimplify-exp10-log10-outer-reassoc/) | `exp10(log10(x))` folds with only outer-call `reassoc`. |
| 016 | [016-instsimplify-log-exp-outer-reassoc](bugs/016-instsimplify-log-exp-outer-reassoc/) | `log(exp(x))` folds with only outer-call `reassoc`. |
| 017 | [017-instsimplify-log2-exp2-outer-reassoc](bugs/017-instsimplify-log2-exp2-outer-reassoc/) | `log2(exp2(x))` folds with only outer-call `reassoc`. |
| 018 | [018-instsimplify-log2-pow2-outer-reassoc](bugs/018-instsimplify-log2-pow2-outer-reassoc/) | `log2(pow(2.0, x))` folds with only outer-call `reassoc`. |
| 019 | [019-instsimplify-log10-exp10-outer-reassoc](bugs/019-instsimplify-log10-exp10-outer-reassoc/) | `log10(exp10(x))` folds with only outer-call `reassoc`. |
| 020 | [020-instsimplify-log10-pow10-outer-reassoc](bugs/020-instsimplify-log10-pow10-outer-reassoc/) | `log10(pow(10.0, x))` folds with only outer-call `reassoc`. |
| 021 | [021-instcombine-div-by-exp-unflagged](bugs/021-instcombine-div-by-exp-unflagged/) | `z / exp(y)` becomes `z * exp(-y)` through an unflagged `exp`. |
| 022 | [022-instcombine-div-by-exp2-unflagged](bugs/022-instcombine-div-by-exp2-unflagged/) | `z / exp2(y)` becomes `z * exp2(-y)` through an unflagged `exp2`. |
| 023 | [023-instcombine-div-by-pow-unflagged](bugs/023-instcombine-div-by-pow-unflagged/) | `z / pow(x, y)` becomes `z * pow(x, -y)` through an unflagged `pow`. |
| 040 | [040-instcombine-ldexp-i64-exponent-truncation](bugs/040-instcombine-ldexp-i64-exponent-truncation/) | `llvm.ldexp.f64.i64(x, K)` multiply fold truncates the exponent to `int`; separate no-FMF FP bug. |

## Non-Integral Pointer Representation Catalog

These are the earlier pointer-full-representation candidates. They are kept
separate and are not counted in the ordinary-target total, because the current
search is prioritizing bugs that manifest with integer pointer types. This
checkout does not currently contain live repro folders for these historical
candidate names, so they are listed without links.

| Historical name | Status |
|-----------------|--------|
| `002-instcombine-ptrtoint-or-null-nonaddress-bits` | deprioritized non-integral/non-address-bit pointer representation candidate |
| `003-instcombine-ptrtoint-icmp-drops-nonaddress-bits` | deprioritized non-integral/non-address-bit pointer representation candidate |
| `004-instcombine-inttoptr-icmp-adds-nonaddress-bits` | deprioritized non-integral/non-address-bit pointer representation candidate |
| `005-instcombine-inttoptr-null-adds-nonaddress-bits` | deprioritized non-integral/non-address-bit pointer representation candidate |
| `007-instcombine-inttoptr-add-ptrtoint-gep-nonaddress-carry` | deprioritized non-integral/non-address-bit pointer representation candidate |
| `008-instsimplify-gep-ptrtoint-provenance-to-inttoptr-constant` | deprioritized non-integral/non-address-bit pointer representation candidate |
| `011-instcombine-ptrtoint-gep-difference-nonaddress-wrap` | deprioritized non-integral/non-address-bit pointer representation candidate |
| `012-instcombine-external-state-inttoptr-ptrtoint-roundtrip` | deprioritized non-integral/external-state pointer representation candidate |
| `013-instcombine-phi-external-state-inttoptr-roundtrip` | deprioritized non-integral/external-state pointer representation candidate |
| `016-instcombine-ptrtoint-eq-nonaddress-constant` | deprioritized non-integral/non-address-bit pointer representation candidate |
| `017-instcombine-ptrtoint-ne-nonaddress-constant` | deprioritized non-integral/non-address-bit pointer representation candidate |
| `018-instcombine-ptrtoint-ult-pair-nonaddress-bits` | deprioritized non-integral/non-address-bit pointer representation candidate |
| `019-instcombine-ptrtoint-ult-nonaddress-constant` | deprioritized non-integral/non-address-bit pointer representation candidate |
| `020-instcombine-inttoptr-ult-pair-adds-nonaddress-bits` | deprioritized non-integral/non-address-bit pointer representation candidate |
| `021-instcombine-inttoptr-eq-nonaddress-constant` | deprioritized non-integral/non-address-bit pointer representation candidate |
| `022-instcombine-inttoptr-ult-nonaddress-constant` | deprioritized non-integral/non-address-bit pointer representation candidate |
| `023-instcombine-ptrtoint-slt-pair-nonaddress-bits` | deprioritized non-integral/non-address-bit pointer representation candidate |
| `024-instcombine-inttoptr-slt-pair-adds-nonaddress-bits` | deprioritized non-integral/non-address-bit pointer representation candidate |
