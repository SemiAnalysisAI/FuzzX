# m153: WholeWaveFunction prologue computes `EXEC = ~entryEXEC` instead of `EXEC = -1`

*Discovery method: code inspection + ISA cross-reference (during
SIFrameLowering 2nd-pass audit).*

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/SIFrameLowering.cpp:1041-1048`
(`SIFrameLowering::emitCSRSpillStores`, WholeWaveFunction branch):

```cpp
if (FuncInfo->isWholeWaveFunction()) {
  // If we have already saved some WWM CSR registers, then the EXEC is
  // already -1 and we don't need to do anything else. Otherwise, set
  // EXEC to -1 here.
  if (!ScratchExecCopy)
    buildScratchExecCopy(LiveUnits, MF, MBB, MBBI, DL,
                         /*IsProlog*/ true,
                         /*EnableInactiveLanes*/ true);   // <-- BUG
  else if (WWMCalleeSavedRegs.empty())
    EnableAllLanes();
}
```

`buildScratchExecCopy(..., EnableInactiveLanes=true)` (line 974-980)
emits `S_XOR_SAVEEXEC_B{32,64} tmp, -1`.  Per AMDGPU ISA:

```
tmp = EXEC
EXEC = -1 XOR EXEC = ~EXEC
```

The in-source comment claims "set EXEC to -1 here," but the actual
result is `EXEC = ~entryEXEC` -- the **bit-inverted** entry mask, not
all-ones.

## Trigger

`amdgpu_gfx_whole_wave` calling convention function with:

* No WWM scratch register spills (`WWMScratchRegs.empty()`)
* No WWM CSR (`WWMCalleeSavedRegs.empty()`)

Both conditions hold easily for a small WWF body that has no
`llvm.amdgcn.strict.wwm` MFMA chains.

`ScratchExecCopy` is null because no earlier WWM-spill loop set it;
the `!ScratchExecCopy` branch runs and emits the bad
`S_XOR_SAVEEXEC` with `-1`.

`SI_WHOLE_WAVE_FUNC_SETUP` pseudo is erased at line 1340
immediately after, so this prologue path is the function's only
EXEC initializer.

## Effect

Function body executes with `EXEC == ~entryEXEC`:

* Lanes that were active at entry become inactive.
* Lanes that were inactive at entry become active.

The whole-wave semantic that the body code relies on is violated
end-to-end -- every per-lane output is on the wrong lanes.

## Reproducer

`reduced.ll`:

```llvm
define amdgpu_gfx_whole_wave i32 @t(i1 %active, ptr addrspace(1) %p, i32 %x) {
  store i32 %x, ptr addrspace(1) %p
  ret i32 0
}
```

`llc -mtriple=amdgcn -mcpu=gfx950 -O2 reduced.ll` emits:

```asm
s_xor_saveexec_b64 s[a:b], -1   ; <-- EXEC = ~entryEXEC, not -1
; body uses EXEC...
```

## Suggested fix

Either pass `EnableInactiveLanes=false` (which would emit
`S_OR_SAVEEXEC ..., -1` -> `EXEC = -1`) or call `EnableAllLanes()`
directly (the saved EXEC is not needed; WWF reads its return EXEC
from the `SI_WHOLE_WAVE_FUNC_RETURN` operand).

Concretely:

```cpp
if (FuncInfo->isWholeWaveFunction()) {
  if (!ScratchExecCopy)
    EnableAllLanes();                                  // <-- fix
  else if (WWMCalleeSavedRegs.empty())
    EnableAllLanes();
}
```

Compare line 1051, where the non-WWF path correctly restores EXEC
from the saved copy after `S_OR_SAVEEXEC` was used in line 1034.

## Why the fuzzer hasn't caught it

* The IR fuzzer rarely emits `amdgpu_gfx_whole_wave` calling
  convention.  Per `MEMORY.md` (Prefer-random-over-idioms), the
  random emitter should generate WWF functions with varying
  WWM-register usage profiles (zero / few / many CSR + scratch).
* The differential O0-vs-O2 oracle won't catch this unless the
  oracle runs in WWF context with masked entry EXEC and compares
  per-lane outputs.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Emits `s_xor_saveexec_b64 ..., -1` for empty-WWM WWF. |
| ROCm 7.1.1 | Same defect. |

## Family

* m149 (SIPreAllocateWWMRegs skips AV-class).
* m152 (getDestEquivalentVGPRClass strips AV-class).
* Same WWM/EXEC management correctness family on gfx950.
