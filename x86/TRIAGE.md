# Triage by Severity (upstream-impact focused)

227 catalog entries. 128 reproducible at default x86 -O2. ~99 source-confirmed only. Tiers below assigned by user-visible impact when the bug fires. Within each tier the bugs are ordered by "report-first" priority.

(16 entries previously listed have been removed: four pure sNaN-quieting losses (112, 115, 116, 117) — known LLVM limitation, not worth filing — three "poison → concrete value" folds (123, 156, 247), which the LangRef explicitly permits: *"It is correct to replace a poison value with an undef value or any value of the type."* — one metadata-loss missed-opt (166: mem2reg `!noundef` lost across PHI, but the "fix" requires adding an `assume(noundef)` which is itself an optimization-blocker) — two LICM `promoteLoopAccessesToScalars` reports (185, 186) that don't survive analysis under LLVM's static-deref / capture-analysis semantics — four `freeze`-CSE reports (136, 187, 188, 194) which on closer reading of LangRef are a valid refinement: source admits both equal- and different-value executions, so a CSE that picks the equal-value execution is a strict narrowing of source nondeterminism (matches the design intent of D75334 / `cc28a754679a`, *"Let EarlyCSE fold equivalent freeze instructions"*) — one DAGCombiner vector-splat-1 report (061) whose theoretical "SRL-by-bitwidth → UNDEF" path is unreachable in practice: `SimplifyVBinOp` scalarizes the splat ahead of the broken `isOneConstant` early-out, so the asm is already correct on upstream and the patch is a strict no-op — and one AtomicExpand InitLoaded report (131) that is illegal under LLVM IR semantics but appears intentionally harmless on x86.)

(Four further entries dropped after Opus-4.7 audit: #002 (DAGCombiner `visitFMinMax` returns sNaN unchanged for `minimumnum(sNaN, qNaN)`) — LangRef's `floatnan` rules explicitly permit "Unchanged NaN propagation", so returning an input sNaN as-is when both inputs are NaN is allowed; the fold matches the spec. #215 / #216 (LowerExpectIntrinsic `handleBrSelExpect` / `handleSwitchExpect` overwriting PGO `!prof`) — intentional MisExpect design (`bac6cd5bf856`): `__builtin_expect` is supposed to override frontend PGO, with `checkFrontendInstrumentation` emitting a `-pgo-warn-misexpect` diagnostic on conflict (see `MisExpect.cpp:15-27`, test `Transforms/PGOProfile/misexpect-branch.ll`). #220 (`patchReplacementInstruction` drops nsw/nuw from kept dominator) — the global drop is required for correctness: `add nsw x,y` is poison on overflow but `extractvalue (sadd.with.overflow x,y), 0` is defined on overflow; RAUW-ing extractvalue users with a still-nsw add gives them poison where source had a defined wrapped value. PR #82935 added the drop to fix miscompile #82884; `Transforms/GVN/pr82884.ll` asserts it.)

---

## S0 — Hard crashes (4)

Compiler ICE / verifier abort on legal IR. These prevent compilation entirely. **File these first; they are unambiguous bugs.**

| # | Bug | Trigger | Fix complexity |
|---|-----|---------|----------------|
| **071** | `opt -passes=codegenprepare` SIGSEGV | any module via opt's new-PM | medium — null-deref in PSI lookup, needs PSI to be materialized or null-guarded |
| **218** | Verifier null-deref on malformed `!prof !"VP"` | hand-crafted IR or bitcode round-trip | trivial — mirror branch-weights null-check |
| **222** | ExpandIRInsts ICE on `<2 x i256> @llvm.fpto{u,s}i.sat` | source-level vector convert with element > 128b | small — add IntrinsicInst case to `scalarize` |
| **227** | AtomicExpandPass crash on `load atomic <2 x i64>` (+cx16) | `_Atomic __int128` vector load in Clang | small — bitcast non-int/non-ptr before cmpxchg, mirror `createCmpXchgInstFun` |

## S1 — Runtime miscompiles (6) — produces wrong runtime value

End-user observable wrong-value miscompiles. Highest non-crash severity.

| # | Bug | What goes wrong |
|---|-----|-----------------|
| **004** | X86 LowerFLDEXP AVX-512F: `<4 x float> @llvm.ldexp` | missing `vcvtdq2ps` → returns x × 1.0 instead of x × 2^exp |
| **011** | LegalizeDAG ldexp.f64.i64 libcall | i64 exponent silently truncated to int → returns wrong magnitude |
| **013** | InstCombine `vector_reduce_mul(sext <n x i1>)` | for odd n with all-true input, returns +1 instead of -1 |
| **155** | frexp.f64.i64 libcall stack-slot overrun | reads 8 bytes from a 4-byte write → uninitialized upper bytes (also info leak) |
| **003 / 110** | X86 GISel UADDE / USUBE inverted carry on multi-word add/sub | wrong upper word — only fires with `-global-isel` (NOT default on x86) |

## S2 — Alive2-falsifiable poison/refinement miscompiles (~20) — turns defined value into poison, or over-infers a flag that makes downstream code poison

These are the strongest correctness-class non-runtime bugs. Several are already-known patterns getting independent confirmation; others are net-new.

**Sound-direction wins (defined → poison) — file as bugs:**

| # | Bug | Fold |
|---|-----|------|
| **195** | InstCombine `ldexp(ldexp(x, INT_MAX), INT_MAX)` | folds to `fmul x, 0.25` (i32 sum wraps to -2 inverting overflow → underflow); should be `+inf` |
| **206** | SimplifyLibCalls `fmod(NaN, 1.0)` | folded to `frem nnan NaN, 1.0` → poison (mis-named `IsNoNan` actually means no-errno) |
| **207** | SimplifyLibCalls `fdim(±Inf, ±Inf)` | folds to qNaN instead of `+0` per C99 |
| **234** | InstSimplify strict-FP `constrained.fadd nnan` fold | drops FE_INVALID exception side-effect (strict-FP semantics violated) |
| **236** | InstCombineSimplifyDemanded `ashr exact → lshr exact` | for `%x=-1`, source returns 255; target returns poison (anti-refinement) |
| **246** | ConstantFolding `ldexp.f64.i64(1.0, 4294967330)` | folds to `2^34` instead of `+inf`; sibling of #011 in const-fold rather than libcall expand |
| **251** | CVP undef-tainted lattice | `select i1 %cmp, i64 undef, i64 1` taints lattice → CVP adds `range(i64 1,3)` AND `add nuw nsw`; both unsound for undef=INT_MAX (matches upstream #114902) |
| **252** | JumpThreading `unfoldSelectInstr` branches on poison | original uses `freeze`; after JT, freeze is gone and `br i1 %maybe_poison` → UB |

**SeparateConstOffsetFromGEP, mem2reg, LICM (default O2 — UB-injection):**

| # | Bug |
|---|-----|
| **181** | SeparateConstOffsetFromGEP `swapGEPOperand` unconditionally `setIsInBounds(true)` — can mark temporarily-OOB GEP as inbounds → guaranteed poison (NOT in default x86 -O3 but in many other pipelines) |

## S3 — Memory-model / atomic / volatile semantic breakage (~45)

`isVolatile()`/`isAtomic()` not checked, syncscope narrowed/widened, atomic ordering dropped. Semantic contract broken even when no observable wrong-value today.

**Volatile silently dropped or moved:**
- **001** `atomicrmw or %p, 0` (idempotent) volatile bit dropped
- **015 / 041** X86AvoidStoreForwardingBlocks no volatile check (load & blocker arms)
- **017** AtomicExpand `widenPartwordAtomicRMW` drops volatile
- **075** GISel matchUndefStore drops volatile/atomic
- **093** AVX512 VMOVS load-fold under masking misses suppress
- **102** X86LowerTileCopy RAX spill MOV without MMO
- **108** DSE partial-merge drops volatile/atomic
- **109** MemCpyOpt processMemSetMemCpyDependence drops volatile memset
- **111** lower-atomic strips volatile/ordering/syncscope (non-default pipeline)
- **114** GVNSink volatile included in hash → merged
- **120, 121, 122** SimplifyCFG sink/hoist merges 2× volatile / seq_cst atomic / atomic loads
- **128** LowerMatrix fuseFlatten drops volatile
- **141** BranchFolder merges volatile + plain store
- **142** MachineLICM hoists volatile/atomic stack-cookie stores
- **143** MemCpyOpt processMemMove drops volatile memset (source-confirmed)
- **148, 190, 191** VectorCombine scalarize{LoadExtract,LoadBitcast,Load} strips atomic / infinite-loops (#191 hang)
- **152, 154** SimplifyCFG sink merges 2× volatile seq_cst {cmpxchg, atomicrmw}
- **182** SimplifyCFG sink merges 2× `fence`

**Atomic ordering / syncscope narrowed or widened:**
- **072** GVN-MSSA computeLoadStoreVN ignores atomic/volatile/ordering/syncscope
- **088** SCEV howManyLessThans unsigned uses signed stride
- **118, 137, 138** SROA drops atomic
- **124, 125** AtomicExpand i128 load/store to cmpxchg drops volatile+syncscope
- **126, 144, 160, 161** LICM promote drops/narrows syncscope (4 variants)
- **132, 133, 174, 175, 176** AtomicExpand drops AAMD on various paths
- **134** AtomicExpand RMW/CAS/Load/StoreToLibcall drops volatile+SSID
- **135** LICM hoists `fence acquire`/`seq_cst` out of loop — collapses N fences to 1
- **184** InstCombine element-atomic memcpy/memset collapses per-byte granularity
- **224** SDAGBuilder visitAtomicRMW/CmpXchg drops `I.getAlign()` + AAMD
- **226, 238** BranchFolding tail-merge: drops atomic ordering / narrows syncscope
- **239** MachineLateInstrsCleanup hasIdentical ignores MMOs (NT lost on merged invariant-load)

## S4 — PGO / profile data corruption (8)

Silently wrong branch weights — affects code layout, inlining, MachineBlockPlacement, MachineOutliner. No wrong code values, but PGO-driven builds get the wrong topology.

| # | Bug |
|---|-----|
| **139** | CGP `splitBranchCondition` passes original weights instead of scaled-down weights |
| **232** | SimpleLoopUnswitch + `SwitchInstProfUpdateWrapper::getSuccessorWeight` zeroes default-case weight when `"expected"` tag present |
| **099** | ImplicitNullChecks `insertFaultingInstr` drops MI flags (FrameSetup/FrameDestroy/TailCall) — affects CFI |
| **018** | CALL_RVMARKER hard-codes SysV preserved mask on Windows — wrong ABI clobber set |
| **036, 045, 076, 077, 030, 046** | Various CFI/EH frame emission gaps (mostly source-confirmed) |

## S5 — Silent codegen asm/MIR wrong (14)

Wrong assembly emitted at the codegen level — observable when reading the asm but no wrong runtime value (memory-model softening) or no value-diff at runtime (NT hint lost → MOVNT replaced by cached MOV).

- **005** X86FixupInstTuning `ProcessShiftLeftToAdd` mutates MI but returns false — pass lies about preservation
- **008** ReturnThunks misses RETI/LRET/IRET only under non-default thunk-extern mitigation — recorded, not prioritized
- **010** LVI-RET misses RETI/LRET/IRET only under non-default LVI-CFI mitigation — recorded, not prioritized
- **009** CET-IBT missing endbr on WinEH funclet entry
- **012** CGP `splitMergedValStore` strips atomic on i64 split
- **014** RESET_FPENV MMO mis-tagged as MOStore on load
- **140** CGP `splitMergedValStore` drops NT/tbaa/alias.scope/noalias
- **151** strict-FP ldexp libcall silent truncation (sibling of #011)
- **240** X86 stack-probe completely skipped for one-page alloca (defeats `-fstack-clash-protection`)
- **357 (in candidates)** BranchFolder drops pcsections
- **231** BranchFolding tail-merge strengthens `nuw` flag (unsound direction)

## S6 — Metadata loss (~84)

Mostly missed-optimizations downstream: `!nontemporal`, `!tbaa`, `!range`, `!invariant.load`, `!noalias`, `!alias.scope`, `!access_group`, `!align`, `!noundef`, `!dereferenceable`, `!unpredictable`, etc., dropped by passes that should have called `combineMetadataForCSE`/`copyMetadataForLoad`/etc.

**Most can be closed by a handful of upstream patches in shared helpers:**

- `Local.cpp combineMetadata` (#219, #229, #230, #287, #288, #289) — the `MD_nontemporal`/`MD_nosanitize`/`MD_alloc_token` arms incorrectly strip K's metadata when J lacks it; the function iterates only K's kinds, dropping J-only kinds
- `Local.cpp dropUBImplyingAttrsAndMetadata` keep-list (#420, #421) — too narrow; affects SimplifyCFG `speculativelyExecuteBB` and `foldTwoEntryPHINode`
- `MachineInstr::isIdenticalTo` ignores MMOs (#141, #237, #239, #357, #358) — affects MachineCSE, BranchFolder, MachineLateInstrsCleanup
- `MachineMemOperand::operator==` omits SuccessOrdering/FailureOrdering/SyncScopeID (#226, #238, #355, #356)
- `DAGCombiner` 4-arg `getLoad`/`getStore` overloads default AAInfo to empty (#196, #198–#199, #221, #224)
- `ScalarizeMaskedMemIntrin` const+dyn-mask paths miss `Load/Store->copyMetadata(*CI)` (#180, #202–#205)
- `SROA` hand-rolled metadata copy lists (#118, #137, #138, #158, #159, #245, #290–#293)
- `LICM promoteLoopAccessesToScalars` per-access metadata copy (#126, #144, #160, #161, #297, #298)
- `InstCombine unpack{Load,Store}ToAggregate` (#211, #212, #245)
- `InstCombine mergeStoreIntoSuccessor` (#221, #312)
- `JumpThreading` only forwards MD_prof (#214, #260–#263, #670)
- `SDAG memcpy/memmove/memset getMemcpyLoadsAndStores` series (#208–#210, #460–#462, #510)

(Full list: bugs #022, #023, #091, #103, #132, #133, #145, #146, #147, #149, #150, #153, #157, #163, #164, #165, #167, #168, #169, #170, #171, #172, #176, #177, #178, #179, #180, #183, #189, #196, #198–#214, #228–#230, #237, #239, #241–#246, #250, #439, #460–#462, #485, #486, #489, …)

## S7 — Source-confirmed / latent (~100)

Reads-correct-as-buggy in source but no opt/llc reproducer constructed (often because the buggy code path is unreachable on current x86 -O2 IR, or requires fuzzed MIR). Useful to bundle as "code-quality / API-tightening" patches.

(Bugs #007, #014, #015–#024, #025–#070, #073, #074, #076–#107 …)

---

## Recommended report-first batch (12 bugs)

Annotated with upstream-issue status as of 2026-05-21:

| # | Bug | Upstream status | Action |
|---|-----|-----------------|--------|
| **218** | Verifier null-deref on `!prof !"VP"` | PR [#145584](https://github.com/llvm/llvm-project/pull/145584) merged, added shape checks but does NOT cover this null-deref. **Residual bug.** | File as new issue |
| **071** | `opt -passes=codegenprepare` SIGSEGV | **Duplicate**: open issue [#173360](https://github.com/llvm/llvm-project/issues/173360), fix PR [#173385](https://github.com/llvm/llvm-project/pull/173385) pending merge | Comment on existing issue / verify the pending PR fixes our case |
| **227** | AtomicExpand `load atomic <2 x i64>` crash | Adjacent PR [#148900](https://github.com/llvm/llvm-project/pull/148900) fixed libcall path but not cmpxchg path at lines 668-687. **Likely residual.** | Re-verify at upstream HEAD; file if still crashing |
| **222** | ExpandIRInsts ICE on vector fpto_sat | No duplicate found. **Novel.** | File as new issue |
| **195** | InstCombine ldexp chain integer overflow | No duplicate. **Novel.** | File |
| **206** | `fmod(NaN)` → poison via mis-named `IsNoNan` | No duplicate. **Novel.** | File |
| **207** | `fdim(±Inf,±Inf)` → qNaN | No duplicate. **Novel.** | File |
| **236** | ashr exact → lshr exact anti-refinement | No duplicate. **Novel** (in-tree test `ashr_can_be_lshr` bakes in the buggy output and needs updating with the fix) | File |
| **251** | CVP undef-tainted lattice | **Duplicate**: open issue [#114902](https://github.com/llvm/llvm-project/issues/114902), still open and unfixed | Comment on existing issue with our `add nuw nsw` additional case |
| **240** | X86 stack-probe skips one-page alloca | No duplicate. **Novel** (security mitigation defeated under `-fstack-clash-protection`) | File |

**Net actionable**: 8 novel issues to file + 2 to comment on existing. The 2 duplicates (#071 #251) are both already known, so they don't need new issues — adding our reproducers as comments still has value.

Then in S3 batch, the most upstream-friendly cluster is the **shared-helper class** of fixes — see `ROOT_CAUSE_PATCHES.md`: 7 PRs in ~225 lines of code close ~39 of the ~84 S6 metadata-loss bugs.
