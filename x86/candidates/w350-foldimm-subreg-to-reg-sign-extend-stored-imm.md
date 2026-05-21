# w350: X86 foldImmediate via SUBREG_TO_REG returns sign-extended imm where zero-extend is required

## Severity
Latent miscompile - depends on whether the underlying `MOV32ri` was constructed with a negative `int64_t` payload (e.g., `addImm(-1)`) versus a non-negative one (e.g., `addImm(0xFFFFFFFF)`).

## Suspicious code
`llvm/lib/Target/X86/X86InstrInfo.cpp:4606-4643` — `X86InstrInfo::getConstValDefinedInReg`:

```cpp
if (MI.isSubregToReg()) {                                  // 4614
  unsigned SubIdx = MI.getOperand(2).getImm();
  MovReg = MI.getOperand(1).getReg();
  if (SubIdx != X86::sub_32bit)
    return false;
  ...
  MovMI = MRI.getUniqueVRegDef(MovReg);                    // 4623
  ...
}
...
if (MovMI->getOpcode() != X86::MOV32ri &&
    MovMI->getOpcode() != X86::MOV64ri &&
    MovMI->getOpcode() != X86::MOV32ri64 &&
    MovMI->getOpcode() != X86::MOV8ri)
  return false;
...
ImmVal = MovMI->getOperand(1).getImm();                    // 4641
return true;
```

The caller is `X86InstrInfo::foldImmediateImpl` (lines 5764-5921) where `Reg` is the SUBREG_TO_REG **outer** (GR64) virtual register. The semantic value of that GR64 is the underlying `MOV32ri` immediate **zero-extended to 64 bits** (because SUBREG_TO_REG's operand 0 is the implicit-zext promise — typically 0).

But `getConstValDefinedInReg` returns `MovMI->getOperand(1).getImm()` as a raw `int64_t`. If the MOV32ri operand was stored as a negative `int64_t` (e.g., `BuildMI(...MOV32ri...).addImm(-1)` as in `X86ISelLowering.cpp:36591` for `__builtin_setjmp`'s mainDstReg), the returned `ImmVal` is `-1` (`0xFFFFFFFFFFFFFFFF`), not the semantic value `0x00000000FFFFFFFF = 4294967295`.

Downstream, in `foldImmediateImpl`:

- Line 5778: `if (Reg in GR64) if (!isInt<32>(ImmVal)) return false;`. For `ImmVal = -1`, `isInt<32>(-1)=true`, **so this passes** rather than rejecting the fold.
- Line 5810-5814: For a `COPY` use with GR64 destination:
  ```cpp
  if (isUInt<32>(ImmVal))
    NewOpc = X86::MOV32ri64;        // zero-extending pseudo
  else
    NewOpc = X86::MOV64ri;          // full 64-bit immediate
  ```
  `isUInt<32>(-1)=false` → picks `MOV64ri`. The COPY use then becomes `MOV64ri %dst, -1`, producing `0xFFFFFFFFFFFFFFFF`.

The original semantic was `0x00000000_FFFFFFFF`. Miscompile.

The check at line 5778 should validate that the value is also `isUInt<32>` when the constant ultimately came from a zero-extending `SUBREG_TO_REG`, or `getConstValDefinedInReg` should zero-extend `ImmVal` before returning when the SUBREG_TO_REG path was taken with the implicit-zext promise.

## Trigger conditions
1. An EH/setjmp/cmpxchg lowering path generates `MOV32ri` with negative `int64_t` payload (e.g., `addImm(-1)`).
2. The resulting GR32 vreg is widened via `SUBREG_TO_REG ..., %vreg32, sub_32bit`.
3. The resulting GR64 vreg is used by a same-class COPY (or other foldable user) that PeepholeOptimizer reaches.
4. PeepholeOptimizer's `foldImmediate` (`PeepholeOptimizer.cpp:1462-1497`) invokes `TII->foldImmediate`.

## Probe IR (does not currently trigger via plain IR because isel uses `MOV32ri64` directly for unsigned 32-bit constants and `MOV64ri` for `-1`-style values):

```llvm
target triple = "x86_64-unknown-linux-gnu"

declare i32 @llvm.eh.sjlj.setjmp(ptr) noreturn

define i32 @sjlj(ptr %buf) {
  %j = call i32 @llvm.eh.sjlj.setjmp(ptr %buf)
  ret i32 %j
}
```

`llc -O2 -mtriple=x86_64-unknown-linux-gnu` currently emits `movl $1, %eax` and does not flow `-1` through SUBREG_TO_REG + COPY. The bug requires a future change (or a non-default path) that produces the MOV32ri/-1 → SUBREG_TO_REG → COPY chain before peephole.

## Root cause summary
`getConstValDefinedInReg` does not zero-extend the inner `MOV32ri` immediate when it was reached via `SUBREG_TO_REG ..., sub_32bit`, leaving the caller to interpret a sign-extended `int64_t` as if it were the zero-extended GR64 value.

## Fix sketch
At line 4641, when `MI.isSubregToReg()` and inner opcode is `MOV32ri` / `MOV32ri64`, mask the returned value:
```cpp
if (MI.isSubregToReg() && (MovMI->getOpcode() == X86::MOV32ri ||
                           MovMI->getOpcode() == X86::MOV32ri64))
  ImmVal = static_cast<uint32_t>(ImmVal);
```
