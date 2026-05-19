# PTX ISA fuzzing backlog

Source baseline:

- NVIDIA PTX ISA 9.2, especially the Instruction Set contents:
  https://docs.nvidia.com/cuda/parallel-thread-execution/contents.html#instruction-set
- Current generator surface:
  `crates/fuzzx-execgen/src/lib.rs`.

This is a planning list, not a claim that every PTX variant is covered.  The
goal is to separate instruction families that are already represented from
families that are plausible next targets and families that need a different
harness before they can be fuzzed safely.  "Candidate" means "can probably be
made deterministic and race-free in the current differential oracle".

## Already fuzzed

| ISA area | Representative instructions | Notes |
| --- | --- | --- |
| Integer arithmetic | `add`, `sub`, `mul`, `mad`, `mul24`, `mad24`, `sad`, `div`, `rem`, `abs`, `neg`, `min`, `max`, `popc`, `clz`, `bfind`, `fns`, `brev`, `bfe`, `bfi`, `szext`, `bmsk`, `dp4a`, `dp2a` | Includes signed/unsigned, 32-bit and many 64-bit forms, register/immediate operands, predication for many forms, and sanitizer rules for division. |
| Extended-precision integer | `add.cc`, `addc`, `sub.cc`, `subc`, `mad.cc`, `madc` | Covered in 32-bit and 64-bit carry-chain forms. |
| Logic and shift | `and`, `or`, `xor`, `not`, `cnot`, `lop3`, `shf`, `shl`, `shr` | Includes immediate and register shift counts where safe; funnel shifts use wrap/clamp-aware generation. |
| Predicate logic | `and.pred`, `or.pred`, `xor.pred`, `not.pred` | Covered in generated instruction streams and in a deterministic entry prologue to keep all predicate-only boolean operators present even in small programs. |
| Comparison and selection | `set`, `setp`, `selp`, `slct` | Covered for integer, predicate, f32/f64, and 64-bit paths; true-value materialization is tracked to avoid known-bug rediscovery. |
| Data movement and conversion | `mov`, `ld`, `ld.global.nc`, `ldu`, `st`, `prefetch`, `prefetchu`, `prmt`, `cvta`, `cvt`, `cvt.pack`, `isspacep` | Covers globals, const, local, shared, generic addresses, vectors, cache operators, prefetch hints, volatile forms, narrow/wide integer conversion, f32/f64 conversion, saturating integer packing, and special registers. |
| Floating point f32/f64 | `testp`, `copysign`, `add`, `sub`, `mul`, `fma`, `div`, `abs`, `neg`, `min`, `max`, `rcp`, `sqrt`, `rsqrt`, `sin`, `cos`, `lg2`, `ex2` | Inputs are sanitized to finite domains; approximate operations are used carefully because approximate results can invalidate an exact-output oracle. |
| Half precision floating point | `add`, `sub`, `mul`, `fma`, `abs`, `neg`, `min`, `max`, `set`, `setp`, `selp.b16`, and conversion chains on `.f16`; arithmetic plus `cvt.rn.f16x2.f32` on `.f16x2` | Deterministic finite constants are emitted in the entry prologue and folded into existing u32 outputs. |
| Packed/subword integer | `add.u16x2`, `add.s16x2`, `min.u16x2`, `max.u16x2`, scalar `.u16`/`.s16`, `mul.wide.u16`, `mad.wide.u16` | Includes `.b16` scratch-register generation and suppressors for known scalar16 families. |
| Memory roundtrips | `ld/st.global`, generic `ld/st`, `ld/st.local`, `ld/st.shared`, `ld.const`, vector loads/stores, cache hints, volatile forms | Memory addresses are bounded to per-thread slices or private local/shared storage. Generic pointers are derived from the existing bounded global output region. |
| Atomics and reductions | `atom.global.{add,exch,cas,inc,dec,min,max,and,or,xor}`, `red.global.{add,inc,dec,min,max,and,or,xor}`, `atom.shared.{add,exch,cas,inc,dec,min,max,and,or,xor}`, `red.shared.{add,inc,dec,min,max,and,or,xor}` | Global forms use per-thread output-slice roundtrips; shared forms use per-thread private shared slots. Post-known profile keeps global min/max suppressed but can exercise add/inc/dec/and and the new shared forms. |
| Uniform memory ordering | `membar.cta`, `membar.gl`, `membar.sys`, `fence.acq_rel.{cta,gpu,sys}`, `fence.sc.{cta,gpu,sys}` | No value result; emitted in uniform instruction stream only, so it cannot deadlock and only constrains memory ordering. |
| Uniform synchronization | `bar.warp.sync`, `bar.sync`, `barrier.sync`, `bar.red`, `barrier.red` | Emitted in the entry prologue with full-warp or full-CTA participation before generated divergent control flow, so all 32 lanes reach the same barrier. |
| Warp collectives | `activemask`, `vote.sync`, `match.sync`, `elect.sync`, `shfl.sync`, `redux.sync` | Full-mask forms are emitted in the entry prologue before generated divergent control flow, so all warp lanes participate and results are deterministic. |
| Device helper calls | `call.uni` to generated `.func` helpers | A deterministic leaf helper is emitted before the entry kernel and called from the uniform prologue, so every lane participates and the return value feeds existing outputs. |
| Control flow | `bra`, `brx.idx`, predicated instructions, structured braces, `ret` | Generator emits arbitrary CFG or structured if/loop shapes with bounded loop counters, plus a bounded prologue branch table whose targets rejoin normally. |
| Special registers and predefined constants | `%tid`, `%ntid`, `%ctaid`, `%nctaid`, `%laneid`, `%nwarpid`, `WARP_SZ`, `%lanemask_*` | Read as deterministic scalar inputs; predicated forms exist for some paths. |

## Candidates

| Priority | ISA area | Candidate instructions | Why it is fuzzable | Main implementation work |
| --- | --- | --- | --- | --- |
| High | More half precision dataflow | In-body `.f16` / `.f16x2` arithmetic, more predicate variants, and randomized f16 conversion chains | The deterministic prologue covers basic arithmetic, scalar compare materialization, predicate-fed selection, and representative conversion chains; richer randomized placement is still plausible with sanitized finite values. | Add reusable half value synthesis, pack/unpack helpers, and exact-or-approx oracle policy. |
| High | More packed conversions | f16/bf16/tf32 conversions and narrower integer pack/unpack forms beyond `cvt.pack` | The deterministic prologue now covers representative saturating `cvt.pack` forms; other packed conversions are still mostly pure dataflow and optimizer-heavy. | Add typed scratch registers and avoid approximate/boundary cases unless marked intentionally inexact. |
| High | More atomics/reductions | 64-bit integer atomics, floating add/exch/CAS where supported, and shared-memory variants beyond the current 32-bit integer set | Private shared slots and per-thread global slices preserve determinism; old/new values can be folded into outputs. | Extend atomic op/type matrix and add type-specific reload/folding logic. |
| High | More warp collective dataflow | Additional in-body uniform-island placements for the existing warp collectives | Full-mask prologue forms are covered; further coverage is plausible if the emitter can guarantee uniform participation. | Add a reusable "warp-uniform island" emitter. |
| Medium | More branch table control flow | In-body `brx.idx` tables | Prologue branch tables are covered; in-body tables are safe if the index is masked to an in-range table and every target rejoins normally. | Generate dense local label tables inside generated CFG or structured regions. |
| Medium | More device helper calls | Multi-argument/multi-return `call`, non-`uni` call spelling in uniform regions, and richer explicit ABI patterns | The basic leaf `.func` prologue call is covered; more call shapes can still stress inliner, ABI lowering, and register passing. | Expand the generated `.func` library and marshal params/returns without recursion. |
| Medium | Uniform synchronization | Barrier arrive variants and additional uniform-region placements | Barriers are safe only when all participating threads reach them. | Add more uniform-only insertion points and avoid divergent/early-exit paths. |
| Medium | Cache policy helpers | `createpolicy`, `applypriority`, `discard` | Mostly compile/optimizer surface; some can be paired with later loads without changing semantics. | Emit valid policy operands and treat as low-oracle/no-output instructions unless paired with memory. |
| Medium | New packed integer types | `.u8x4` / `.s8x4` `add`, `sub`, `min`, `max`, `neg` | PTX 9.2 calls these out as new instruction types; byte-lane operations are deterministic and likely optimizer-heavy. | Add byte-lane packing helpers and suppressor flags distinct from existing video/packed16 paths. |
| Medium | More special registers | `%cluster_*`, `%is_explicit_cluster`, `%lanemask_*` variants not yet covered | Some are deterministic within one launch or at least stable enough if only compared between opt levels in the same launch shape. | Decide which are stable across O0/O3 runs; avoid values that can differ between separate launches unless normalized. |
| Low | Additional predicate-only algebra | More in-body `and.pred`, `or.pred`, `xor.pred`, `not.pred`, and dual-destination `setp` forms | Pure dataflow and the basic boolean operators are already covered. | Increase in-body coverage density and add variant-specific suppressors if needed. |
| Low | Floating edge modes | More `.ftz`, `.sat`, `.rn/rz/rm/rp` combinations, f64 approximations where available | Mostly already covered for f32/f64; remaining forms are incremental. | Expand mnemonic tables and keep exact-output hazards quarantined. |

## Not candidates for the current harness

| ISA area | Representative instructions | Why not in the current fuzzer |
| --- | --- | --- |
| Texture instructions | `tex`, `tld4`, `txq`, `istypep` | Need texture/sampler declarations and host-side texture-object setup. The current executor only passes raw global input/output buffers. |
| Surface instructions | `suld`, `sust`, `sured`, `suq` | Need CUDA surface objects or surface state, plus careful bounds and format setup outside the current ABI. |
| Tensor map operations | `tensormap.replace`, tensor-map operands for bulk/tensor copies | Need tensor-map descriptors and ABI plumbing that the simple three-parameter kernel does not provide. |
| Asynchronous copy protocol | `cp.async`, `cp.async.bulk`, `cp.reduce.async.bulk`, waits/commits, tensor/bulk prefetch variants | Correctness depends on commit/wait groups, shared-memory staging, barriers, and proxy semantics. It is fuzzable only with a separate protocol-aware async-copy harness. |
| Mbarrier lifecycle | `mbarrier.init`, `mbarrier.arrive`, `mbarrier.test_wait`, `mbarrier.try_wait`, `mbarrier.expect_tx`, `mbarrier.complete_tx` | Requires a correctly initialized shared mbarrier object, phase tracking, uniform participation, and deadlock avoidance. Current random control flow is not safe for this. |
| CTA/cluster barriers | `bar`, `barrier`, `barrier.cluster` | Unstructured or divergent generation can deadlock if not every required thread/CTA reaches the same barrier. Only uniform-island support would make a subset candidate. |
| Cluster launch/control and dependent-grid features | `clusterlaunchcontrol.*`, `griddepcontrol`, cluster-rank data movement | Need cluster launch dimensions or CUDA graph dependency setup, which the executor does not currently expose. |
| Multimem operations | `multimem.ld_reduce`, `multimem.st`, `multimem.red`, `multimem.cp.async.bulk` | Require multimem addresses and launch/runtime setup outside the current single global-buffer ABI. |
| Warp/matrix and tensor-core operations | `wmma.*`, `mma`, `mma.sp`, `ldmatrix`, `stmatrix`, `movmatrix`, `wgmma.*`, `tcgen05.*` | Need warp- or warpgroup-cooperative fragments, strict register tuple shapes, shared-memory matrix layouts, and uniform participation. This should be a separate matrix harness, not ad hoc scalar fuzzing. |
| TensorCore 5th generation memory management | `tcgen05.alloc`, `tcgen05.dealloc`, `tcgen05.ld`, `tcgen05.st`, `tcgen05.cp`, `tcgen05.wait`, `tcgen05.mma*` | Requires tensor memory allocation permits, descriptors, CTA-pair/peer-CTA issue rules, and specialized synchronization. Unsafe in the current per-thread scalar generator. |
| Deprecated warp collectives | old `vote`, old `shfl` | The ISA keeps deprecated forms, but new `.sync` forms are better targets. Deprecated forms may have target restrictions and are not worth first-pass coverage. |
| Volatile/profiling special registers | `%warpid`, `%smid`, `%nsmid`, `%gridid` | Values can differ between the separate O0 and O3 kernel launches, and `%warpid` is explicitly volatile. They are diagnostics/profiling inputs rather than stable exact-output oracle data. |
| Trap/debug/system side effects | `trap`, `brkpt`, profiler/debug hooks if exposed by target PTX version | They intentionally alter execution or debugging state and do not produce a meaningful O0-vs-O3 value oracle. |

## Suggested next slices

1. Add half-precision scalar arithmetic and conversion, starting with exact-ish
   forms and excluding approximations until the oracle is defined.
2. Add a warp-uniform island emitter, then implement `shfl.sync`, `vote.sync`,
   `match.sync`, `activemask`, and `redux.sync`.
3. Add more `brx.idx` insertion points after the next long clean fuzz interval.
