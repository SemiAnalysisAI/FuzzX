# NVPTX LLVM bug hunt

*Human-written portion*

These are the results of a code-inspection bug hunt against the LLVM **NVPTX
backend** (the PTX code generator), in the spirit of the sibling `../x86` hunt.
Not fuzzing — Claude was asked to read the backend and find miscompiles.

Scope (per the request):
- **Interesting:** miscompiles (emitted PTX whose semantics differ from the
  input IR), compiler segfaults, and — grudgingly — assertion failures.
- **Not interesting:** "cannot select"/unsupported errors, dropped metadata,
  missed optimizations, perf. The NVPTX-specific IR passes that act like a
  target CodeGenPrepare (and so don't run for CPU targets) are in scope.

Everything below here is machine-generated. Good luck.

------------

**Status: 66 confirmed bugs** — 17 miscompiles, 28 compiler crashes
(assert/abort/OOB/stack-overflow), and 21 cases of silently-emitted invalid/unassemblable PTX (ptxas-rejected; validated against the PTX ISA since no local ptxas was available). Found by eight
fan-out rounds of code reading. Every entry was reproduced with a freshly built
NVPTX-enabled `llc`/`opt`; the miscompiles and early crashes were additionally put
through an independent adversarial "try to refute it" pass before being listed here.

**Upstream:** 11 fix PRs filed against `llvm/llvm-project` so far — **10 merged**
(#001, #002, #003/#020, #007, #014, #022, #028, #029, #033, #058) and **1 open**
(#023). See [Upstream PRs](#upstream-prs) for links.

The adversarial pass earned its keep: it correctly **rejected** several
plausible-looking candidates (PTX `min`/`max` *do* order signed zeros per ISA
8.7; `cvt.ftz.f32.f16` only flushes f32 operands, not the f16 source;
inline-asm width mismatches are user error; `prmt …, -0x1U` is valid PTX). Those
are listed at the bottom.

### Headline miscompiles (default `llc`, ordinary IR)
- **#001** — `fptoui`/`fptosi` of a float to `i1` compiles to `(a == 0.0)` (cross-checked: x86 uses `cvttss2si` low bit; `opt` folds `fptoui 1.5→i1`=1, NVPTX gives 0). *(fixed: [#200718](https://github.com/llvm/llvm-project/pull/200718), merged)*
- **#002** — `sext(shl nsw x, bitwidth-1)` becomes `mul.wide.s` by a *negative* constant → product negated. *(fixed: [#200924](https://github.com/llvm/llvm-project/pull/200924), merged)*
- **#003 / #020** — a guarded shift folded to a PTX clamp-shift that only sees the low 32 bits of the amount. *(fixed: [#201165](https://github.com/llvm/llvm-project/pull/201165), merged)*
- **#022** — scoped `atomicMax_block`/`Min` on **unsigned** lowers to a **signed** `atom.max.s32`. *(fixed: [#200735](https://github.com/llvm/llvm-project/pull/200735), merged)*
- **#031** — a `volatile` load gets tagged `!invariant.load` and lowered to `ld.global.nc`, dropping volatile.
- **#056** — a `blockaddress` nested in an aggregate global is emitted as all-zero bytes (relocation dropped).
- **#058** — a non-power-of-2 vector struct field (`<3 x i32>`) drops its tail padding, shifting later fields (trailing `i32` lands at byte 12 not 16). *(fixed: [#201246](https://github.com/llvm/llvm-project/pull/201246), merged)*

### Upstream PRs

Fixes filed against `llvm/llvm-project` for bugs above. *Merged* has landed on
`main`; *open* is in review.

| # | PR | Status |
|---|-----|--------|
| 001 | [#200718](https://github.com/llvm/llvm-project/pull/200718) — Fix fptosi/fptoui to i1 | merged |
| 002 | [#200924](https://github.com/llvm/llvm-project/pull/200924) — Fix sext(shl nsw x, topbit) miscompile | merged |
| 003 / 020 | [#201165](https://github.com/llvm/llvm-project/pull/201165) — PerformSELECTShiftCombine drops high bits of a wide guarded shift amount | merged |
| 007 | [#201177](https://github.com/llvm/llvm-project/pull/201177) — Fix aggregate load/store lowering for (potentially) overlapping copies | merged |
| 014 | [#201184](https://github.com/llvm/llvm-project/pull/201184) — Print the full value of e.g. an i65 global | merged |
| 022 | [#200735](https://github.com/llvm/llvm-project/pull/200735) — Remove nvvm scoped atomic intrinsics; use atomicrmw/cmpxchg | merged |
| 023 | [#201217](https://github.com/llvm/llvm-project/pull/201217) — Properly emit narrow ptrtoint in aggregate initializers | open |
| 028 | [#201245](https://github.com/llvm/llvm-project/pull/201245) — NVVMIntrRange: handle maxntid > UINT32_MAX | merged |
| 029 | [#201220](https://github.com/llvm/llvm-project/pull/201220) — Handle symbol-relative integer initializers in aggregates (+ follow-up [#201473](https://github.com/llvm/llvm-project/pull/201473) removing `sub`) | merged |
| 033 | [#200732](https://github.com/llvm/llvm-project/pull/200732) — Respect FTZ flag when lowering atomicrmw fadd | merged |
| 058 | [#201246](https://github.com/llvm/llvm-project/pull/201246) — Pad non-power-of-2 vectors in structs properly | merged |

## Tools
- LLVM source: `/Users/justinlebar/code/vm-shared/llvm` (= `~/code/llvm`), HEAD `6d2a90bd8bf3` at session start.
- `llc`/`opt` with NVPTX enabled: `/Users/justinlebar/code/llvm2/build/bin/{llc,opt}`
  (llvm2 reconfigured with `NVPTX` added to `LLVM_TARGETS_TO_BUILD`, same commit,
  "Optimized build with assertions").
- Default target: `nvptx64-nvidia-cuda` (`-mtriple=nvptx64`), tuned per-bug with `-mcpu=sm_XX`.

## Layout
- `bugs/NNN-short-name/` — one folder per confirmed bug: `NOTES.md` (mechanism,
  root cause, observed-vs-expected, verification log), `repro.ll` (minimal IR),
  `cmd.sh` (exact reproduce command; `TOOL=/path/to/llc ./cmd.sh` to override).
- `candidates/` — round-1 pre-triage notes. `scratch/` — the workflow scripts and raw finder/verifier JSON.

## How to reproduce one
```sh
cd bugs/001-fptoui-fptosi-to-i1-compares-eq-zero && ./cmd.sh
```

## Bug catalog

| # | Bug | Kind | Reachable via |
|---|-----|------|---------------|
| 001 | [fptoui-fptosi-to-i1-compares-eq-zero](bugs/001-fptoui-fptosi-to-i1-compares-eq-zero/) — `fptoui`/`fptosi` of any float to `i1` is lowered as `(a == 0.0)` instead of `trunc(a)&1` | **miscompile** | default llc -O0/-O2 |
| 002 | [combinemulwide-sext-shl-topbit-negates](bugs/002-combinemulwide-sext-shl-topbit-negates/) — `sext(shl nsw x, bitwidth-1)` folded to `mul.wide.s` with a negative constant, negating the result | **miscompile** | default llc -O1+ |
| 003 | [selectshiftcombine-i64-clamp-drops-high32](bugs/003-selectshiftcombine-i64-clamp-drops-high32/) — guarded i64 shift folded to PTX clamp shift using only the low 32 bits of the shift amount | **miscompile** | default llc (after legalize) |
| 004 | [int-to-bf16-double-rounding](bugs/004-int-to-bf16-double-rounding/) — `sitofp/uitofp i32/i64 -> bfloat` double-rounds via f32, giving the wrong correctly-rounded result | **miscompile** | llc -mcpu=sm_80 (sm<90 \|\| ptx<78) |
| 005 | [byval-atomicrmw-marked-readonly](bugs/005-byval-atomicrmw-marked-readonly/) — byval kernel param written via atomicrmw/cmpxchg is wrongly marked `readonly`+`grid_constant`, no local copy | **miscompile** | default llc, sm_70+ |
| 006 | [vaarg-sub-i16-stride-mismatch](bugs/006-vaarg-sub-i16-stride-mismatch/) — `va_arg` of i8/i1 advances the va_list by 2 bytes while the caller packs at 1-byte stride | **miscompile** | default llc |
| 007 | [aggr-loadstore-overlap-forward-copy](bugs/007-aggr-loadstore-overlap-forward-copy/) — overlapping aggregate `load;store` lowered to a forward memcpy loop (no overlap/direction handling) | **miscompile** | default llc (aggregate >=128B) |
| 008 | [lowerargs-byval-unhandled-ptr-use-crash](bugs/008-lowerargs-byval-unhandled-ptr-use-crash/) — byval kernel param used by icmp/freeze/atomicrmw/cmpxchg hits `llvm_unreachable` in NVPTXLowerArgs | crash (abort/UB) | default llc |
| 009 | [ldu-global-nonconst-align-crash](bugs/009-ldu-global-nonconst-align-crash/) — `llvm.nvvm.ldu.global.*` with a non-constant alignment operand crashes `getTgtMemIntrinsic` | crash (assert/UB) | default llc |
| 010 | [asmprinter-subbyte-vector-global-oob](bugs/010-asmprinter-subbyte-vector-global-oob/) — global with a nested sub-byte vector (`<8 x i4>`) overflows the AsmPrinter constant buffer | crash (OOB write) | default llc |
| 011 | [asmprinter-subbyte-splat-oob](bugs/011-asmprinter-subbyte-splat-oob/) — sub-byte vector splat ConstantInt overflows the AsmPrinter constant buffer (latent path) | crash (OOB write) | needs -use-constant-int-for-fixed-length-splat |
| 012 | [nvvm-intr-range-maxclusterrank-overflow](bugs/012-nvvm-intr-range-maxclusterrank-overflow/) — `nvvm.maxclusterrank=UINT32_MAX` overflows a 32-bit APInt in NVVMIntrRange | crash (assert/UB) | opt -passes=nvvm-intr-range (clang IR pipeline) |
| 013 | [tcgen05-ld-offset-i32-truncation](bugs/013-tcgen05-ld-offset-i32-truncation/) — `tcgen05.ld.16x32bx2` i64 offset immediate is built as i32, asserting (or truncating in release) | crash (assert) / miscompile in release | llc -mcpu=sm_100a |
| 014 | [asmprinter-large-int-global-drops-high-byte](bugs/014-asmprinter-large-int-global-drops-high-byte/) — module-scope `iN` global (N>64, N not a multiple of 8) drops its top partial byte (silent wrong constant) | **miscompile** | default llc |
| 015 | [tcgen05-st-offset-i32-truncation](bugs/015-tcgen05-st-offset-i32-truncation/) — `tcgen05.st.16x32bx2` i64 offset immediate built as i32 in `SelectTcgen05St` (sibling of #013) | crash (assert) / miscompile in release | llc -mcpu=sm_100a |
| 016 | [asmprinter-subbyte-vector-non-divisor-assert](bugs/016-asmprinter-subbyte-vector-non-divisor-assert/) — vector global with sub-byte element width not dividing 8 (`<2 x i3>`, `<8 x i5>`) asserts in NVPTXAsmPrinter | assertion (release: graceful fatal_error) | default llc |
| 017 | [tryldg-oob-invariant-atomic-load](bugs/017-tryldg-oob-invariant-atomic-load/) — invariant atomic load (e.g. const __restrict__ + atomic) routed to tryLDG reads operands 3/4 of a 2-operand AtomicSDNode → OOB/assert | crash (assert/UB) | default llc, sm_70+ |
| 018 | [replaceimagehandle-select-selp-unreachable](bugs/018-replaceimagehandle-select-selp-unreachable/) — a texture/surface handle produced by `select` (→ SELP) hits `llvm_unreachable` in NVPTXReplaceImageHandles | crash (abort/UB) | default llc, CUDA tex/surf |
| 019 | [internal-ptx-kernel-byval-overalign](bugs/019-internal-ptx-kernel-byval-overalign/) — an `internal` ptx_kernel with a byval param trips `assert(!isKernelFunction)`; release over-aligns the host-filled `.param` slot | crash (assert) / miscompile in release | default llc |
| 020 | [selectshiftcombine-crosswidth-guard-drop](bugs/020-selectshiftcombine-crosswidth-guard-drop/) — PerformSELECTShiftCombine matches a guard comparing a wider (i64) amount against a narrower (i32) shift, dropping the guard | **miscompile** | default llc |
| 021 | [subbyte-vector-param-oob](bugs/021-subbyte-vector-param-oob/) — `<N x i1>` param is declared ceil(N/8) bytes but elements are loaded/stored at byte offsets 0..N-1 (out of the slot) | **miscompile** | default llc |
| 022 | [scoped-atomic-minmax-always-signed](bugs/022-scoped-atomic-minmax-always-signed/) — scoped atomic min/max (`atomicMax_block` etc. on unsigned) always lowers to signed `atom.max.s32`/`.s64` | **miscompile** | default llc, sm_70+ |
| 023 | [asmprinter-ptrtoint-narrow-int-aggregate](bugs/023-asmprinter-ptrtoint-narrow-int-aggregate/) — aggregate initializer with `ptrtoint(@sym to iN)` (N*8<ptrsize) emits a full 8-byte pointer, dropping the following field (or asserting) | **miscompile** | default llc |
| 024 | [nvvm-intr-range-maxntid-zero-dim](bugs/024-nvvm-intr-range-maxntid-zero-dim/) — `nvvm.maxntid`/`reqntid` with a 0 dimension builds an empty ConstantRange → assert in NVVMIntrRange | crash (assert/UB) | opt -passes=nvvm-intr-range (clang IR pipeline) |
| 025 | [lowercall-argouts-size-assert](bugs/025-lowercall-argouts-size-assert/) — scalar integer call argument hits `assert(ArgOuts.size()==1)` in LowerCall | crash (assert/UB) | default llc |
| 026 | [kernel-param-nonfundamental-width-invalid-ptx](bugs/026-kernel-param-nonfundamental-width-invalid-ptx/) — ptx_kernel integer param of non-fundamental width (i48/i24/i3...) emits `.param .u48` etc. — not a legal PTX type (ptxas rejects) | other (invalid PTX) | default llc |
| 027 | [scoped-fp-atom-add-drops-noftz-invalid-ptx](bugs/027-scoped-fp-atom-add-drops-noftz-invalid-ptx/) — scoped f16/bf16 `llvm.nvvm.atomic.add.gen.f.{cta,sys}` emits `atom.cta.add.f16` without the mandatory `.noftz` (ptxas rejects; non-scoped path is correct) | other (invalid PTX) | default llc, sm_90+/f16 any |
| 028 | [nvvm-intr-range-maxntid-product-overflow](bugs/028-nvvm-intr-range-maxntid-product-overflow/) — `nvvm.maxntid` whose dimension product exceeds UINT_MAX is truncated u64→unsigned before the 1024 clamp → tid/ntid folded to tiny wrong constants (or empty-range assert) | **miscompile** | opt -passes=nvvm-intr-range / clang -O2 IR pipeline |
| 029 | [asmprinter-ptrtoint-addsub-aggregate-crash](bugs/029-asmprinter-ptrtoint-addsub-aggregate-crash/) — aggregate initializer element `add/sub(ptrtoint(@sym), C)` hits `llvm_unreachable("unsupported integer const type")` in bufferLEByte | crash (abort/UB) | default llc |
| 030 | [asmprinter-fp128-aggregate-crash](bugs/030-asmprinter-fp128-aggregate-crash/) — an `fp128` element nested in an array/struct global hits `llvm_unreachable("unsupported type")` in bufferLEByte (top-level fp128 is fine) | crash (abort/UB) | default llc |
| 031 | [taginvariant-volatile-load-dropped](bugs/031-taginvariant-volatile-load-dropped/) — NVPTXTagInvariantLoads tags a `load volatile` (from a noalias readonly arg) `!invariant.load`, lowering it to `ld.global.nc` and dropping volatile | **miscompile** | default llc, sm_70+ |
| 032 | [scoped-atom-cas-underguarded-invalid-ptx](bugs/032-scoped-atom-cas-underguarded-invalid-ptx/) — scoped `atom.cas` patterns carry no subtarget predicate: `atom.cta.cas.b16` emitted below sm_70/PTX6.3 and `.cta/.sys` scope below sm_60 (ptxas rejects) | other (invalid PTX) | default llc, sm_50/sm_60 |
| 033 | [atomicrmw-fadd-f32-forced-ftz](bugs/033-atomicrmw-fadd-f32-forced-ftz/) — `atomicrmw fadd float` always lowers to `atom.add.f32`, which hardware-flushes subnormals even in the default IEEE denormal mode (no non-flushing variant chosen) | **miscompile** | default llc |
| 034 | [kernel-param-i65-i127-crash](bugs/034-kernel-param-i65-i127-crash/) — ptx_kernel integer parameter of width 65–127 bits hits `llvm_unreachable("Integer too large")` in the AsmPrinter param printer | crash (assert/abort) | default llc |
| 035 | [knownbits-prmt-recursion-overflow](bugs/035-knownbits-prmt-recursion-overflow/) — `computeKnownBitsForPRMT` recurses with un-incremented `Depth`, defeating the recursion-depth guard → stack overflow on a long prmt chain (and exponential compile time on a branching one) | crash (stack overflow / exponential) | default llc |
| 036 | [nonsync-vote-no-arch-guard-invalid-ptx](bugs/036-nonsync-vote-no-arch-guard-invalid-ptx/) — non-`.sync` `vote.{all,any,uni,ballot}` patterns lack an upper-arch predicate (sibling non-sync `shfl` has `hasSHFL`); `vote.ballot.b32` is emitted on sm_70+/PTX≥6.4 where it was removed | other (invalid PTX) | llc -mcpu=sm_70+ |
| 037 | [cvta-param-no-arch-guard-invalid-ptx](bugs/037-cvta-param-no-arch-guard-invalid-ptx/) — `cvta.param`/`cvta.to.param` patterns carry no subtarget predicate — emitted below sm_70/PTX7.7, and a nonexistent `cvta.param.u32` form on 32-bit | other (invalid PTX) | default llc |
| 038 | [add-sub-ftz-sat-modifier-order-invalid-ptx](bugs/038-add-sub-ftz-sat-modifier-order-invalid-ptx/) — f32 `nvvm.add/sub.*.ftz.sat` emit `add.rn.sat.ftz.f32` (`.sat` before `.ftz`) — wrong PTX modifier order (f16 add, f32 fma all use the correct `.ftz.sat`) | other (invalid PTX) | default llc |
| 039 | [sustp-subword-invalid-ptx](bugs/039-sustp-subword-invalid-ptx/) — formatted surface store `sust.p` with i8/i16 value emits `sust.p.1d.b8`/`.b16` — the formatted store only has a `.b32` form in PTX | other (invalid PTX) | default llc |
| 040 | [atom-const-param-addrspace-invalid-ptx](bugs/040-atom-const-param-addrspace-invalid-ptx/) — `atomicrmw`/`cmpxchg` on a const(AS4)/param(AS101) pointer emits `atom.const.*`/`atom.param.*` — atom only supports .global/.shared/generic | other (invalid PTX) | default llc |
| 041 | [asmprinter-half-bfloat-global-crash](bugs/041-asmprinter-half-bfloat-global-crash/) — a module-scope scalar `half`/`bfloat` global crashes `printFPConstant` (`llvm_unreachable "unsupported fp type"`) — these are common CUDA types | crash (abort/UB) | default llc |
| 042 | [asmprinter-x86fp80-ppcfp128-global-crash](bugs/042-asmprinter-x86fp80-ppcfp128-global-crash/) — a top-level `x86_fp80`/`ppc_fp128` global crashes `printModuleLevelGV` (`"type not supported yet"`) | crash (abort/UB) | default llc |
| 043 | [asmprinter-x86fp80-ppcfp128-nested-crash](bugs/043-asmprinter-x86fp80-ppcfp128-nested-crash/) — an `x86_fp80`/`ppc_fp128` element nested in an array/struct global crashes `bufferLEByte` (`"unsupported type"`) | crash (abort/UB) | default llc |
| 044 | [asmprinter-largeint-constexpr-global-crash](bugs/044-asmprinter-largeint-constexpr-global-crash/) — a >64-bit integer global with an unfolded ConstantExpr initializer crashes `bufferAggregateConstant` (`"unsupported constant type"`) | crash (abort/UB) | default llc |
| 045 | [ctordtor-nonstruct-element-crash](bugs/045-ctordtor-nonstruct-element-crash/) — `llvm.global_ctors`/`global_dtors` with a `zeroinitializer`/`poison` array element crashes `cast<ConstantStruct>` in NVPTXCtorDtorLowering | crash (assert/UB) | default llc |
| 046 | [selection-unhandled-addrspace-crash](bugs/046-selection-unhandled-addrspace-crash/) — a load/store/atomicrmw through an unhandled pointer address space hits an `llvm_unreachable` in NVPTXISelDAGToDAG | crash (abort/UB) | default llc |
| 047 | [lowerargs-scalable-byval-typesize-crash](bugs/047-lowerargs-scalable-byval-typesize-crash/) — a scalable-vector `byval` kernel param hits the TypeSize scalable→fixed `reportFatalInternalError` in `copyByValParam` | crash (fatal/UB) | default llc, sm_90 |
| 048 | [narrowfp-cvt-missing-sm80-guard-invalid-ptx](bugs/048-narrowfp-cvt-missing-sm80-guard-invalid-ptx/) — narrow-fp cvt intrinsics (`f2bf16`/`ff2bf16x2`/`f2f16`/`ff2f16x2`, incl `.relu`) use standalone Pats lacking the instruction's sm_80 guard → `cvt.*.bf16/.bf16x2/.relu.f16` emitted below sm_80 | other (invalid PTX) | llc -mcpu<sm_80 |
| 049 | [mma-m16n8k8-bf16-tf32-underguard-invalid-ptx](bugs/049-mma-m16n8k8-bf16-tf32-underguard-invalid-ptx/) — bf16/tf32 `mma.sync.m16n8k8` is under-guarded (FragA-only predicate) and emitted on sm_75/PTX6.x where it requires a newer target | other (invalid PTX) | llc -mcpu=sm_75 |
| 050 | [mma-m16n8k32-fp8-underguard-invalid-ptx](bugs/050-mma-m16n8k32-fp8-underguard-invalid-ptx/) — e4m3/e5m2 `mma.sync.m16n8k32` (f8f6f4) is guarded only at PTX 8.4 but requires PTX 8.7 (the m16n8k16 sibling clause does not cover k32) | other (invalid PTX) | llc -mcpu=sm_89 -mattr=+ptx84 |
| 051 | [ldst-shared-cluster-no-arch-guard-invalid-ptx](bugs/051-ldst-shared-cluster-no-arch-guard-invalid-ptx/) — `ld`/`st` to shared::cluster (AS 7) emit `.shared::cluster` with no sm_90/PTX7.8 guard on the LD/ST classes | other (invalid PTX) | default llc |
| 052 | [cvta-shared-cluster-no-arch-guard-invalid-ptx](bugs/052-cvta-shared-cluster-no-arch-guard-invalid-ptx/) — `addrspacecast` to/from shared::cluster emits `cvta.shared::cluster` below sm_90 — the C++ `getMachineNode` path bypasses the `[hasClusters]` predicate | other (invalid PTX) | default llc |
| 053 | [atom-b128-sys-scope-ptx83-invalid-ptx](bugs/053-atom-b128-sys-scope-ptx83-invalid-ptx/) — default-scope `atomicrmw xchg`/`cmpxchg i128` emits `atom.sys.{exch,cas}.b128` at PTX 8.3, but `.sys` on `.b128` atom requires PTX 8.4 | other (invalid PTX) | llc -mcpu=sm_90 -mattr=+ptx83 |
| 054 | [cmpxchg-local-addrspace-invalid-ptx](bugs/054-cmpxchg-local-addrspace-invalid-ptx/) — `cmpxchg` on a local (AS 5) pointer emits `atom.local.cas.b{32,64,128}` — atom does not support the `.local` state space (distinct from #040, which dismissed local) | other (invalid PTX) | default llc |
| 055 | [store-const-addrspace-invalid-ptx](bugs/055-store-const-addrspace-invalid-ptx/) — a scalar `store` through a constant-space (AS 4) pointer emits `st.const`, which is not a valid PTX store | other (invalid PTX) | default llc |
| 056 | [blockaddress-aggregate-emitted-as-zeros](bugs/056-blockaddress-aggregate-emitted-as-zeros/) — a `blockaddress` nested in an aggregate global is silently emitted as all-zero bytes (the block-address relocation is dropped) | **miscompile** | default llc |
| 057 | [blockaddress-global-crash](bugs/057-blockaddress-global-crash/) — a scalar `ptr`/`iN` global initialized to `blockaddress` (or `ptrtoint(blockaddress)`) crashes the AsmPrinter (`llvm_unreachable`) | crash (abort/UB) | default llc |
| 058 | [asmprinter-nonpow2-vector-field-padding](bugs/058-asmprinter-nonpow2-vector-field-padding/) — a non-power-of-2 vector (`<3 x i32>` etc.) as a non-last struct field/array element drops its tail padding, placing the following field at the wrong offset (e.g. trailing `i32` at byte 12 instead of 16) | **miscompile** | default llc |
| 059 | [aggrcopies-scalable-vector-typesize-crash](bugs/059-aggrcopies-scalable-vector-typesize-crash/) — a scalable-vector `load;store` pair makes NVPTXLowerAggrCopies convert a scalable TypeSize to fixed → `reportFatalInternalError` (distinct site from #047) | crash (fatal/UB) | default llc -O0 |
| 060 | [replaceimagehandle-nonsymbol-load-crash](bugs/060-replaceimagehandle-nonsymbol-load-crash/) — on an nvcl target, a tex/surf handle loaded from a non-symbol (register/alloca) address asserts in NVPTXReplaceImageHandles (`isSymbol()`) | crash (assert/UB) | llc nvptx64-unknown-nvcl |
| 061 | [atom-shared-cluster-no-arch-guard-invalid-ptx](bugs/061-atom-shared-cluster-no-arch-guard-invalid-ptx/) — `atomicrmw`/`cmpxchg` on shared::cluster (AS 7) emits `atom.shared::cluster.*` with no sm_90/PTX7.8 guard (distinct atom path from #051's ld/st) | other (invalid PTX) | default llc |
| 062 | [asmprinter-opaque-unsized-global-crash](bugs/062-asmprinter-opaque-unsized-global-crash/) — an `external global` of an opaque/unsized type crashes `emitPTXGlobalVariable` (DataLayout `getAlignment` assert on an unsized type) | crash (assert/UB) | default llc |
| 063 | [cpreduce-async-bulk-tensor-no-guard-invalid-ptx](bugs/063-cpreduce-async-bulk-tensor-no-guard-invalid-ptx/) — `cp.reduce.async.bulk.tensor.*` is custom-selected via `getMachineNode`, bypassing its `[hasPTX<80>,hasSM<90>]` Requires → emitted on pre-Hopper targets | other (invalid PTX) | llc -mcpu<sm_90 |
| 064 | [atomicrmw-scope-qualifier-ptx50-invalid-ptx](bugs/064-atomicrmw-scope-qualifier-ptx50-invalid-ptx/) — generic `atomicrmw`/`cmpxchg` emits a `.cta/.gpu/.sys` scope qualifier on sm_60/sm_61 (default PTX 5.0), but atom scopes require PTX ISA 6.0 (`getAtomicScope` checks only sm, not PTX version) | other (invalid PTX) | llc -mcpu=sm_60/sm_61 |
| 065 | [cvt-tf32-relu-satfinite-order-invalid-ptx](bugs/065-cvt-tf32-relu-satfinite-order-invalid-ptx/) — `cvt` to tf32 with `.relu`+`.satfinite` emits `cvt.rn.relu.satfinite.tf32.f32` — wrong qualifier order (tf32 grammar requires `.satfinite` before `.relu`; an existing test locks in the wrong output) | other (invalid PTX) | llc -mcpu=sm_100a -mattr=+ptx86 |
| 066 | [f16-round-ops-no-sm53-guard-invalid-ptx](bugs/066-f16-round-ops-no-sm53-guard-invalid-ptx/) — f16 `ceil/floor/trunc/rint/nearbyint/roundeven` set Legal unconditionally (skipping `setFP16OperationAction`), emitting native `cvt.rpi.f16.f16` etc. on sm_50/sm_52 where f16 math needs sm_53 | other (invalid PTX) | llc -mcpu=sm_50/sm_52 |

## Bug families

Many bugs share a root area (a fix usually wants to cover the whole cluster):
- **NVPTXAsmPrinter constant/global emission** — #010, #011, #014, #016, #023, #029, #030, #041, #042, #043, #044, #056, #057, #058, #062 (unhandled FP/sub-byte/large-int/ptr/blockaddress/opaque types; non-pow2-vector and ptrtoint layout).
- **NVVMIntrRange range bounds** — #012, #024, #028 (APInt/ConstantRange overflow from un-clamped attribute values).
- **NVPTXLowerArgs / byval / param-space** — #005, #008, #019, #021, #047 (`ArgUseChecker` no-op default; param-slot sizing; scalable byval).
- **Scoped / address-space / arch-guard atomics** — #022, #027, #032, #040, #053, #054, #061, #064 (signedness, `.noftz`, missing subtarget predicates, illegal state spaces, PTX-version scope guards).
- **Feature-predicate / arch-validity (emits PTX the target rejects)** — #036, #037, #048, #049, #050, #051, #052, #055, #063, #065, #066 (missing/too-low `hasSM`/`hasPTX` guards; C++ `getMachineNode` paths that bypass `Requires<>`).
- **TagInvariantLoads → tryLDG** — #017, #031 (atomic/volatile loads wrongly treated as invariant).
- **Immediate-width truncation in DAGToDAG** — #013, #015 (`tcgen05` i64 offset built as i32).
- **PerformSELECTShiftCombine** — #003, #020 (wide shift-amount guard dropped).
- **NVPTXReplaceImageHandles** — #018, #060 (unhandled handle-def opcode / non-symbol load).

A large share of the post-#033 finds are the **"backend emits unassemblable PTX"** class
(an instruction/qualifier/type/address-space the declared target rejects). These are
real backend defects on valid IR, but are a notch below the miscompiles/crashes in
severity, and — lacking a local `ptxas` — are validated against the PTX ISA + strong
in-tree corroboration (sibling guards/orderings) rather than executed. They are marked
`other (invalid PTX)` in the catalog.

## Method

Eight fan-out rounds of code-reading subagents (orchestrated with the `Workflow`
tool), each followed by an empirical verify pass (build a minimal repro, run the
built `llc`/`opt`); the miscompiles and the early crashes additionally went through
an independent adversarial "try to refute it" pass that re-derives IR and PTX
semantics by hand. Each later round was told to read this README and avoid the
already-found bugs, then enumerate *every* instance in its assigned seam.

1. **Round 1** — ~30 regions of the backend (custom lowering, DAG combines, ISel
   selection, the NVPTX IR passes, TableGen patterns, AsmPrinter constants).
2. **Round 2** — ~12 *bug classes* abstracted from round 1 (corroborated 8).
3. **Round 3** — deep dives + under-explored files (image handles, MC InstPrinter,
   branch analysis, inline-asm, atomic/cvt/min-max patterns, knownbits).
4. **Rounds 4–5** — ABI/returns, scoped atomics, `getTgtMemIntrinsic` casts,
   sreg-range folding, feature predicates, FP-atomic expansion.
5. **Rounds 6–8** — broad + exhaustive sweeps of the intrinsic-pattern files,
   feature-predicate / arch-validity gaps, address-space validity, immediate-width
   truncation, qualifier ordering, and crash-hunting across the passes / AsmPrinter
   / MC layer. (Yields shifted toward the "invalid-PTX" class as the clean-miscompile
   vein was mined out — see Bug families.)

Scripts: `scratch/find*_workflow.js`, `scratch/verify_workflow.js`.

## Investigated and rejected (not a bug)

- **f16 WMMA fragments use a half-size `MemVT`** — genuinely undersized, but harmless: NVPTX leaves `useAA()==false` and the `wmma` MI carries no MMO / is a chain barrier, so no reordering can occur.
- **`llvm.minimumnum`/`maximumnum` → bare PTX `min`/`max`** — *correct*: PTX ISA 8.7 §9.7.3.11–12 states `min`/`max` order signed zeros (`+0.0 > -0.0`), matching IEEE-754-2019 minimumNumber/maximumNumber.
- **`cvt.ftz.f32.f16` on `fpext half→float`** — *correct*: PTX `.ftz` applies only to single-precision (`.f32`) operands/results, so it does not flush the `f16` source; result equals the non-ftz conversion.
- **Inline-asm wrong-width constraint (`"=r"` on an i64)** — user error / documented target-independent behavior (LangRef "register class too small"); reproduces identically on x86.
- **`printHexu32imm` prints `prmt …, -0x1U`** — ugly but valid PTX: unary-minus of a `u64` literal truncates to `0xFFFFFFFF`, same `prmt` result.
- **InstCombine NVVM f16 min/max/fma FTZ fold** — could not reproduce an observable codegen difference; dropped.
- **`tryBFE` `GoodBits` uses i32 width** — missed optimization only; emitted `bfe` is always correct.
