# m134: `v_bitop3_b32` selector stale-slot patch only resets `LHSBits`, not `RHSBits` -- wrong truth table

*Discovery method: random IR fuzzing reduced by DCE to 6 ops.*  Sibling
shape to the m071/m072/m073 "wrong truth table after recursive slot
mutation" family in `AMDGPUISelDAGToDAG.cpp`.

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/AMDGPUISelDAGToDAG.cpp:4413-4450`
(the recursive bitop3 selector).

When the LHS recursion succeeds and the RHS recursion further mutates
the same src slots, the patch at line 4448 resets only
`LHSBits = LHSBitsOrig` and `NumOpcodes = 0`.  It does NOT reset
`RHSBits`.  The function then returns
`(1, LHSBitsOrig OP RHSBits_recursed, SrcBeforeRecurse)` where the two
bit-vectors describe inconsistent slot semantics: `LHSBitsOrig` was
computed pre-recursion (against the original src triple), while
`RHSBits` was computed during the RHS recursion (against a mutated
src triple).  The resulting truth-table immediate encodes the wrong
function.

The bug manifests via the bitop3 selector picking an operand
decomposition where one slot is a *derived* function of the others
(e.g. `src[0] = ((src[1] ^ src[2]) | a)`), then computing the truth
table as if the three sources were independent.

## Reproducer

`reduced.ll`:

```llvm
define amdgpu_kernel void @t(ptr addrspace(1) %in, ptr addrspace(1) %out) {
  %a = load i32, ptr addrspace(1) %in, align 4
  %b = load i32, ptr addrspace(1) %in_b, align 4    ; in[1]
  %c = load i32, ptr addrspace(1) %in_c, align 4    ; in[2]

  %v0     = xor i32 %c, %b
  %v1     = or  i32 %v0, %a
  %v2     = and i32 %v1, %v0           ; absorption: v2 == v0
  %not_v2 = xor i32 %v2, -1
  %v7     = or  i32 %not_v2, %v0
  %r      = xor i32 %v2, %v7           ; mathematically ~(c^b)

  store i32 %r, ptr addrspace(1) %out
  ret void
}
```

Mathematical reduction:
* `v2 = (v0 | a) & v0 = v0` (absorption).
* `r = v2 ^ (~v2 | v0) = ~v2 | ~v0 = NAND(v2, v0) = NAND(v0, v0) = ~v0 = ~(c^b)`.

For `a=0x12345678, b=0xCAFEBABE, c=0xDEADBEEF`:
* Correct: `~(c^b) = 0xEBACFBAE`.
* O0 (buggy bitop3): `0xFD89EDC7`.
* O2 (other fold path): `0xEBACFBAE`.

`run_ll_reproducer.sh` output:

```
input=0x12345678 O0=0xfd89edc7 O2=0xebacfbae mismatch=true
```

O0 asm:

```asm
v_bitop3_b32 v2, s0, v1, v2 bitop3:0x5f      ; = ~(s0 & v2)
```

`bitop3:0x5f` is `~(a & c)` per the truth table (`f(a,b,c) = ~(a & c)`).
With `s0 = (c^b)|a, v1 = c, v2 = b`, this computes `~(((c^b)|a) & b)`,
not the intended `~(c^b)`.  The selector substituted `b` and `c` for
slot 2 instead of keeping the `v0 = c^b` slot.

## Suggested fix

In the stale-slot patch (lines 4413-4450) also reset `RHSBits` whenever
the mutation invariant is violated:

```cpp
if (LHSStale || RHSStale) {
  LHSBits = LHSBitsOrig;
  RHSBits = RHSBitsOrig;        // <-- ADD THIS
  NumOpcodes = 0;
  // ... rebuild slot decomposition from scratch ...
}
```

A safer alternative is an unconditional "if any slot mutated, revert
and return non-folded" fallback:

```cpp
if (Src.getNode() != SrcBeforeRecurse.getNode())
  return {0, 0, SDValue()};   // bail; let the default selector run
```

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Reproduces (O0 emits buggy bitop3). |
| ROCm 7.2.3 | Reproduces. |
| ROCm staging | Reproduces. |

All three campaign toolchains exhibit the bug.

## Why the fuzzer hasn't caught it

* The bitop3 selector is an ISel pattern (fires at all opt levels), so
  the O0-vs-O2 oracle would catch this *if* O2 took the same fold.
  But O2's earlier combines reduce the expression differently (often
  to a single `v_xor` chain after absorption simplification), so the
  bitop3 fold only fires on the O0 path.
* The specific 4-op tail `v2 ^ ((~v2) | v0)` (where `v2` is derived
  from `v0`) is rare in the random IR pool.
* Per `MEMORY.md` (Prefer-random-over-idioms), the fuzzer's random
  emitter should bias toward 4-6-deep bitwise expressions over a
  small set of source registers where intermediate values get reused
  -- this would surface this and adjacent bitop3 bugs.

## Discovery method detail

Found via random IR fuzzing (seed 1188, n_ops=11), reduced by DCE +
inlining to 6 ops.  Algorithm simulator (Python bitop3 truth-table
solver) confirmed the asm computes `~(((c^b)|a) & b)` and not the
intended `~(c^b)`.
