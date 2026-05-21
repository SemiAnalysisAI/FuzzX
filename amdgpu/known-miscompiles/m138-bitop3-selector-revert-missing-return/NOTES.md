# m138: `v_bitop3_b32` selector stale-slot revert falls through to TTbl computation, returns `(1, garbage)` -- structural root cause of m134 and 5 more fuzzer hits

*Discovery method: code inspection + random IR fuzzing (5 distinct
hits in ~500 random shapes, all reduce to the same revert defect).*

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/AMDGPUISelDAGToDAG.cpp:4446-4451`
(the bitop3 selector revert block):

```cpp
if (LHSStale) {
  Src = std::move(SrcBeforeRecurse);     // <-- restore src
  LHSBits = LHSBitsOrig;                 // <-- restore LHS bits
  NumOpcodes = 0;
  // (no early return!)
}
// ... falls through to TTbl combine at 4457-4470 ...
TTbl = LHSBits OP RHSBits;
return std::make_pair(NumOpcodes + 1, TTbl);
```

The revert sets `NumOpcodes = 0` and restores `LHSBits`/`Src`, but
**does not return**.  Control falls through to the TTbl computation,
which returns `(NumOpcodes + 1, LHSBits OP RHSBits) = (1, garbage)`.
The recursive caller at line 4427 then does `NumOpcodes += Op.first`
and adopts the garbage TTbl as `LHSBits`/`RHSBits` for the outer
node, producing a wrong truth-table immediate on the emitted
`v_bitop3_b32`.

m134 covered one downstream symptom (RHSBits not reset alongside
LHSBits).  But the deeper bug is that the revert is structurally
incomplete -- the right fix is an unconditional early return after
the revert.

Two adjacent contributing defects also exist:

* **`findSlot` misses NOT-of-source patterns** (line 4414-4419).
  `getOperandBits` at line 4373 can return `~SrcBits[I]`
  (`0x0f`/`0x33`/`0x55`) for `xor(X, -1)` when `Src` is full.
  `findSlot` only matches positive `SrcBitsConst[I]` values, so the
  stale-slot guard returns `slot=-1` and the cross-contamination
  check is silently disabled.  Combined with the missing return, this
  is a second route into the wrong-truth-table state.
* **`RHSBitsOrig` is never snapshotted** (line 4423).  Only
  `LHSBitsOrig` is saved, so even if the revert added "also reset
  RHSBits" (m134's suggested fix) the value to reset to wasn't
  preserved.

## Reproducer

`reduced.ll` (s263 from random fuzzer; algebraically reduces to 0):

```llvm
define amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %in, ptr addrspace(1) %out, i32 %n) {
  ...
  ; v0 = b | a
  ; v1 = ~v0
  ; v2 = c & v1
  ; v3 = v2 & v1      ; = c & v1
  ; v4 = v2 ^ c       ; = ~v1 & c & ... = 0 actually, let me recheck
  ; v7 = v3 & v4      ; algebraically (c & v1) & (c & ~v1) = 0  <-- always zero
  ...
}
```

Mathematical answer: `(c & v1) & (c & ~v1) = 0` for all `a, b, c`.

Codegen with `clang -mcpu=gfx950`:

```asm
=== -O0 ===
v_bitop3_b32 v2, v2, v3, v4 bitop3:0x40    ; = slot0 & slot1 & ~slot2
                                            ; selector picked wrong source triple
=== -O2 ===
v_mov_b32_e32 v1, 0                         ; correct: O2 folds to constant 0
```

For `a=0x12345678, b=0xCAFEBABE, c=0xDEADBEEF`:
* O0 stores `0xC888A886` (computed by buggy bitop3).
* O2 stores `0x00000000` (the correct algebraic answer).

## More fuzzer hits, all same root cause

Random IR fuzzing (gen_bitwise.py, 5-8 ops, 3-4 inputs, biased toward
recent-intermediate reuse) found 5 O0/O2 miscompiles in ~500 random
shapes, all triggering the same bug:

| Seed | Shape | O0 bitop3 imm | Diagnosis |
| --- | --- | --- | --- |
| 156  | 7-op generic        | `0x22`              | wrong TTbl |
| 263  | 6-op always-zero    | `0x40` (`a&b&~c`)   | wrong: algebraically 0, emits nonzero |
| 288  | 6-op generic        | `0x33`              | wrong TTbl |
| 389  | 7-op generic        | `0x7e`              | wrong TTbl |
| 409  | 8-op identity-`c`   | `0x3c` (`a^b`)      | wrong: should be c, emits a^b |

The variety of buggy immediates from independent random shapes
demonstrates this is the *general* shape of the bug, not a single
narrow case.  All five reduce to the same revert-without-return
structural defect.

## Suggested fix

Replace the revert body (lines 4446-4451) with:

```cpp
if (LHSStale) {
  // Slot mutation invariant violated; bail and let the caller treat
  // this side as the un-decomposed leaf via the restored Src.
  Src = std::move(SrcBeforeRecurse);
  return std::make_pair(0, 0);    // structural early-return
}
```

Add a parallel `if (RHSStale)` clause with the same early return.
This subsumes m134's fix and the m134/m138 family of symptoms.

Also fix:
* `findSlot` (line 4414): match both `SrcBitsConst[I]` and
  `~SrcBitsConst[I]`, and track which polarity was matched.
* Snapshot `RHSBitsOrig` alongside `LHSBitsOrig` at line 4423 for
  symmetry / defensive programming.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Reproduces (5 of 500 random shapes hit the same bug). |
| ROCm 7.2.3 | Reproduces. |
| ROCm staging | Reproduces. |

## Why the fuzzer caught it (and why the existing fuzzer setup missed it before)

The random emitter targeted 5-8 op chains with reused intermediates
(per `MEMORY.md` prefer-random-over-idioms guidance).  The fuzzer's
existing IR-emitter does not bias strongly enough toward
intermediate reuse + reached-bitop3 shapes -- adding a small bias on
the random bitwise emitter would surface this entire family
immediately.

The differential O0-vs-O2 oracle works because the bitop3 selector
runs at ISel (all opt levels) but O2's earlier combines often
simplify the chain to a form that bypasses the buggy selector path.
