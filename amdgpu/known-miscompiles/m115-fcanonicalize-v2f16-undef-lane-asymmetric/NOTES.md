# m115: `performFCanonicalizeCombine` v2f16 undef-lane fixup is asymmetric -- low lane stays undef when high lane is non-constant

*Discovery method: code inspection.*

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/SIISelLowering.cpp:15910-15915`
(`performFCanonicalizeCombine`, v2f16 build_vector path):

```cpp
if (NewElts[0].isUndef()) {
  if (isa<ConstantFPSDNode>(NewElts[1]))                    // <-- guard
    NewElts[0] = isa<ConstantFPSDNode>(NewElts[1])
                     ? NewElts[1]
                     : DAG.getConstantFP(0.0f, SL, EltVT);  // <-- unreachable
}
```

The inner ternary is **dead**: the outer `if` requires `NewElts[1]` to
be a `ConstantFPSDNode`, so the ternary's false branch
(`getConstantFP(0.0f, ...)`) can never execute.  When the *other* lane
is a non-constant (e.g. `FCANONICALIZE x` of a runtime value), the
guard at line 15910 fails and `NewElts[0]` stays `undef`.

The transformed `build_vector(undef, fcanon(x))` then lets the low
lane decay to arbitrary register bits at codegen, defeating the
contract that `fcanonicalize(undef) -> qNaN` (line 15868 of the same
function, which handles the all-undef case).

The parallel branch at 15917-15921 (for `NewElts[1].isUndef()`) is
correctly written:

```cpp
if (NewElts[1].isUndef()) {
  NewElts[1] = isa<ConstantFPSDNode>(NewElts[0])
                   ? NewElts[0]
                   : DAG.getConstantFP(0.0f, SL, EltVT);  // <-- this one IS reachable
}
```

So lane 0 vs lane 1 are handled asymmetrically: lane 0 only falls back
to a sibling-constant; lane 1 falls back to `0.0` if the sibling isn't
constant.

## Reproducer

`reduced.ll`:

```llvm
define amdgpu_kernel void @k(ptr addrspace(1) %out, half %x) {
  %iv = insertelement <2 x half> poison, half %x, i32 1
  %cv = call <2 x half> @llvm.canonicalize.v2f16(<2 x half> %iv)
  %bc = bitcast <2 x half> %cv to i32
  store i32 %bc, ptr addrspace(1) %out
  ret void
}
```

Codegen with `clang -mcpu=gfx950 -denormal-fp-math=preserve-sign`:

```asm
=== -O0 ===
s_lshl_b32 s2, s2, 16
v_pk_max_f16 v1, s2, s2           ; low half = 0x0000 (from lshl)

=== -O2 ===
v_mov_b32_e32 v1, s2
v_max_f16_sdwa v1, s2, v1 dst_sel:WORD_1
                                  ; low half = raw low 16 bits of s2
                                  ; (passthrough, no canonicalize)
```

For `s2` containing `0x7E017C01` (high half = 0x7E01 which is a
sNaN-flushed canonical value, low half = 0x7C01 which is sNaN):

* O0: low half = `0x0000`, high half = canonicalized `0x7E01`.
* O2: low half = `0x7C01` (raw sNaN -- not canonicalized!), high half
  = canonicalized `0x7E01`.

Swapping the insertelement to use `i32 0` (so the OTHER lane is undef,
hitting the correct fallback branch) yields `v_max_f16_e64 v1, s2, s2`
at O2 -- proving the asymmetry is the bug.

## Suggested fix

Match the symmetric branch at line 15917-15921:

```cpp
if (NewElts[0].isUndef()) {
  NewElts[0] = isa<ConstantFPSDNode>(NewElts[1])
                   ? NewElts[1]
                   : DAG.getConstantFP(0.0f, SL, EltVT);
}
```

(Drop the outer `if (isa<ConstantFPSDNode>(NewElts[1]))` guard.)

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Reproduces (low lane non-canonical at O2). |
| ROCm 7.1.1 (`/opt/rocm-7.1.1/lib/llvm/bin/clang-20`) | Same dead-ternary bug. |

## Related: `isCanonicalized` recursion arity bug (latent)

`SIISelLowering.cpp:15567, 15577, 15618, 15625-26, 15631, 15639, 15642-43, 15653, 15661`:
recursive calls pass `MaxDepth-1` into the `SDNodeFlags` parameter slot
(non-explicit `SDNodeFlags(unsigned)` ctor at `SelectionDAGNodes.h:444`).
`MaxDepth` is never decremented -- it keeps its default value `5`.

Currently latent (the smuggled bitmask never sets `NoNaNs`), but one
flag re-ordering away from a real miscompile.  Note here, separate
fix.

## Why the fuzzer hasn't caught it

* The FP emitter rarely emits `insertelement v2f16 poison, x, 1`
  patterns followed by `llvm.canonicalize.v2f16`.
* The interpreter oracle treats `undef`/`poison` lanes as `0.0`, which
  happens to agree with the O0 codegen here but not the O2 codegen.
* Per `MEMORY.md` (Prefer-random-over-idioms), the right hook is to
  weight `insertelement v2f16 poison, x, {0,1}` higher in the random
  emitter and ensure `llvm.canonicalize.v2f16` consumers see those.
