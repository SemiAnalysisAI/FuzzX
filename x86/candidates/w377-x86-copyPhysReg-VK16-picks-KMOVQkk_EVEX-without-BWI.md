# w377: X86InstrInfo::copyPhysReg selects `KMOVQkk_EVEX` for VK16 copies when EGPR is on and BWI is off - illegal opcode

## Component
`llvm/lib/Target/X86/X86InstrInfo.cpp` - `X86InstrInfo::copyPhysReg` mask-register branch.

## Where
- `llvm/lib/Target/X86/X86InstrInfo.cpp:4363-4367`

```cpp
4363  // All KMASK RegClasses hold the same k registers, can be tested against
4364  // anyone.
4365  else if (X86::VK16RegClass.contains(DestReg, SrcReg))
4366    Opc = Subtarget.hasBWI() ? (HasEGPR ? X86::KMOVQkk_EVEX : X86::KMOVQkk)
4367                             : (HasEGPR ? X86::KMOVQkk_EVEX : X86::KMOVWkk);
```

## Bug
The two arms of the BWI ternary differ only by what's chosen when `HasEGPR` is true. On the `!hasBWI()` arm:
- `HasEGPR == true` -> `KMOVQkk_EVEX`
- `HasEGPR == false` -> `KMOVWkk` (correct - VK16 fits in a 16-bit kmov)

But `KMOVQkk_EVEX` is defined under `Predicates = [HasBWI, HasEGPR, In64BitMode]` (see `llvm/lib/Target/X86/X86InstrAVX512.td:2700-2709`). Specifically:

```td
let Predicates = [HasBWI, HasEGPR, In64BitMode] in {
  ...
  defm KMOVQ : avx512_mask_mov<0x90, 0x90, 0x91, "kmovq", VK64, v64i1, i64mem, "_EVEX">,
               EVEX, TB, REX_W;
  ...
}
```

`KMOVQ` (and its `_EVEX` form) requires AVX-512 BWI. Using `KMOVQkk_EVEX` on a subtarget without BWI produces a `MachineInstr` whose opcode is gated by a predicate the subtarget does not satisfy. The `MCInst` will eventually be emitted to assembly as `kmovq`, which the assembler will accept (it just emits the bytes), but the resulting binary contains an instruction that is undefined on a non-BWI CPU.

## Trigger conditions
Requires:
- `Subtarget.hasAVX512()` and `Subtarget.hasEGPR()` (APX) and **not** `Subtarget.hasBWI()`.
- A copy between two `VK16` physical regs (mask register class).

APX (EGPR) without BWI is a legal feature combination - APX is orthogonal to BWI. On a CPU configured `-mattr=+avx512f,+egpr` (no `+bwi`), VK16 copies will silently take the buggy branch.

## Repro hypothesis
A function that forces the regalloc to copy a vk16 mask between physical registers. Inline asm with explicit clobber list often does, or a pattern that exhausts low mask registers.

```ll
target triple = "x86_64-unknown-linux-gnu"

define void @ksp(ptr %p) {
entry:
  %m = load <16 x i1>, ptr %p
  call void asm sideeffect "nop", "~{k0},~{k1},~{k2},~{k3},~{k4},~{k5},~{k6}"()
  %m2 = and <16 x i1> %m, %m
  store <16 x i1> %m2, ptr %p
  ret void
}
```

`llc -O2 -mtriple=x86_64-unknown-linux-gnu -mattr=+avx512f,+egpr` should exercise the path - if `copyPhysReg` decides any vk16->vk16 physreg copy, you should see a `kmovq` in the asm where a `kmovw` would be correct (BWI is required for `kmovq`).

Note: bringing the copy through `copyPhysReg` requires the register allocator to insert it; not all IR patterns will do so. Adversarial register pressure on mask registers (above) increases the chance.

## Why this slipped
The two ternary arms look almost identical at a glance:
```cpp
Subtarget.hasBWI() ? (HasEGPR ? X86::KMOVQkk_EVEX : X86::KMOVQkk)
                   : (HasEGPR ? X86::KMOVQkk_EVEX : X86::KMOVWkk);
                            // ^^^^^^^^^^^^^^^^^^ wrong arm: should be KMOVWkk_EVEX
```

The `HasEGPR` ternary in the no-BWI branch should select `X86::KMOVWkk_EVEX` (mirror of `KMOVWkk`, which is BWI-independent), not `KMOVQkk_EVEX`.

`KMOVWkk_EVEX` is defined under `Predicates = [HasAVX512, HasEGPR, In64BitMode]` (`X86InstrAVX512.td:2697-2699`):
```td
let Predicates = [HasAVX512, HasEGPR, In64BitMode] in
  defm KMOVW : avx512_mask_mov<0x90, 0x90, 0x91, "kmovw", VK16, v16i1, i16mem, "_EVEX">,
               EVEX, TB;
```

## Severity
Latent. APX + AVX-512F + no-BWI is supportable but uncommon; observability depends on the user emitting binaries for such a target.

## Fix sketch
```cpp
else if (X86::VK16RegClass.contains(DestReg, SrcReg))
  Opc = Subtarget.hasBWI() ? (HasEGPR ? X86::KMOVQkk_EVEX : X86::KMOVQkk)
                           : (HasEGPR ? X86::KMOVWkk_EVEX : X86::KMOVWkk);
```

## Confidence
High that the code is mistyped (the no-BWI arm picking `KMOVQkk_EVEX` is inconsistent with the otherwise-symmetric `KMOVWkk` fall-through). Triggering it on a real binary requires the AVX512F+EGPR-without-BWI configuration.
