# m109: `SIFixSGPRCopies::tryMoveVGPRConstToSGPR` uses raw `S_MOV_B64` for 64-bit imm, truncating high 32 bits

*Discovery method: code inspection.*  Sibling of the `isSafeToFoldImmIntoCopy`
helper at line 386 of the same file, which correctly uses
`S_MOV_B64_IMM_PSEUDO` for the same case.

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/SIFixSGPRCopies.cpp:887`:

```cpp
unsigned MoveOp = MoveSize == 64 ? AMDGPU::S_MOV_B64 : AMDGPU::S_MOV_B32;
BuildMI(*MI.getParent(), MI, ..., get(MoveOp), DstReg).add(*SrcConst);
```

When the source VGPR is defined by `V_MOV_B64_PSEUDO <non-inline 64-bit imm>`
and is fed into a uniform-result PHI / REG_SEQUENCE, this helper rewrites
the imm into a raw `S_MOV_B64`.  `S_MOV_B64` only encodes a 32-bit literal
(the high 32 bits are silently dropped at encoding time).

The sibling helper `isSafeToFoldImmIntoCopy` (line 386) correctly uses
`S_MOV_B64_IMM_PSEUDO`, which expands into the proper two-half move
sequence.  Line 887 is missing that pseudo and is the asymmetry.

A secondary defect at the same line: `MoveSize` other than 32/64 (e.g.,
true16 / 96 / 128 paths) silently falls into `S_MOV_B32`, which is
malformed for the wider `DstReg` allocated at line 689
(`DestRC = TRI->getEquivalentSGPRClass(SrcRC)`).

## Reproducer

`reduced.mir` (in this directory) builds a uniform-result PHI in `bb.1`
with a `vreg_64_align2` operand defined by
`V_MOV_B64_PSEUDO 0x123456789ABCDEF0` in `bb.0`, an `S_LSHR_B64`
self-cycle to keep the PHI uniform-result through SIFixSGPRCopies.

`llc -mtriple=amdgcn-amd-amdhsa -mcpu=gfx950 -run-pass=si-fix-sgpr-copies reduced.mir`:

```
%12:sreg_64 = S_MOV_B64 1311768467463790320
```

Then full asm:

```
s_mov_b64 s[0:1], 0x123456789abcdef0   ; encoding: [0xff,0x01,0x80,0xbe,0xf0,0xde,0xbc,0x9a]
```

The ASM printer pretty-prints the original immediate but the actual
**encoding** is a single 32-bit literal slot.  Disassembly (the ground
truth):

```
s_mov_b64 s[0:1], 0x9abcdef0
```

The high 32 bits `0x12345678` are silently zero-extended away.  Loaded
value at runtime is `0x000000009abcdef0`, not `0x123456789abcdef0`.

## Suggested fix

Mirror `isSafeToFoldImmIntoCopy`:

```cpp
unsigned MoveOp =
    MoveSize == 64 ? AMDGPU::S_MOV_B64_IMM_PSEUDO : AMDGPU::S_MOV_B32;
```

Plus an early-out if `MoveSize` is neither 32 nor 64 (skip the rewrite
for wider classes; they can't be re-expressed by a single MOV).

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Reproduces (encoding loses high 32 bits). |
| ROCm 7.2.3 (`/opt/rocm-7.2.3/lib/llvm/bin/llc`) | Reproduces (not HEAD-only). |

## Why the fuzzer hasn't caught it

* Existing `fix-sgpr-copies` MIR tests in tree use only inline 64-bit
  imms (0, -1), which happen to round-trip through `S_MOV_B64` correctly.
* The IR fuzzer rarely emits a `V_MOV_B64_PSEUDO` of a non-inline 64-bit
  constant feeding a uniform PHI from a divergent block.
* Per `MEMORY.md` (Prefer-random-over-idioms), the right hook is to bias
  the i64 constant pool toward values with both halves non-inline
  (e.g. `0x12345678_9ABCDEF0`) and to emit i64 / `<2 x i32>` PHIs that
  combine a divergent-block VGPR producer with a uniform sibling
  operand.

## v2i32 sibling

The `v2i32` PHI shape gets no special handling — it's treated as a
64-bit class and falls into the same `MoveSize == 64` branch, so the
exact same bug applies when the PHI op is `V_MOV_B64_PSEUDO` of a
`<i32 K1, i32 K2>` bitcast.

## `AV_MOV_B64_IMM_PSEUDO` adjacent missing-case

`isSafeToFoldImmIntoCopy` (line 378-388) handles `V_MOV_B32_e32`,
`AV_MOV_B32_IMM_PSEUDO`, `V_MOV_B64_PSEUDO` — but **not**
`AV_MOV_B64_IMM_PSEUDO` (`SIInstructions.td:168`, `isMoveImm=1`).
Returns false for it instead of folding, so a 64-bit AV imm becomes a
`V_READFIRSTLANE_B32` pair rather than an `S_MOV_B64_IMM_PSEUDO`.
Sub-optimal, not wrong; separate cleanup.
