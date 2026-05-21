# X86DynAllocaExpander: isPushPop() only recognizes 32/64 PUSH/POP of the legacy GPR set

File: llvm/lib/Target/X86/X86DynAllocaExpander.cpp:122-138

```
122  static bool isPushPop(const MachineInstr &MI) {
123    switch (MI.getOpcode()) {
124    case X86::PUSH32r:
125    case X86::PUSH32rmm:
126    case X86::PUSH32rmr:
127    case X86::PUSH32i:
128    case X86::PUSH64r:
129    case X86::PUSH64rmm:
130    case X86::PUSH64rmr:
131    case X86::PUSH64i32:
132    case X86::POP32r:
133    case X86::POP64r:
134      return true;
135    default:
136      return false;
137    }
```

Reasoning: `computeLowerings()` treats `isPushPop()` instructions as
"touches the tip of the stack" and resets `Offset = 0`, then walks the
subsequent DynAllocas with that knowledge. The set deliberately omits
`PUSHF*`, `POPF*`, `PUSH16r`, `POP16r`, `PUSHA*`, `POPA*`, `LEAVE*`,
and the EVEX/APX `PUSH2`/`POP2` family (`PUSH2`, `PUSH2P`, `POP2`,
`POP2P`) introduced for APX. APX `PUSH2` pushes two GPRs at once (16
bytes) and obviously touches the new SP region; failing to recognize it
means a DynAlloca that immediately follows a `PUSH2` in the same block
will be analysed with a stale `Offset = INT32_MAX` from prior modifying
SP code, and may be lowered to a non-probing `Sub`. On Windows targets
(stack probing required for allocations beyond a single page) this can
leave a guard page unprobed.

Repro sketch:
- `-mtriple=x86_64-pc-windows-msvc -mattr=+egpr -mattr=+push2pop2`
  (or whatever APX feature gates PUSH2 — `-mattr=+ppx`). Construct a
  function whose epilogue contains a `PUSH2`/`POP2` pair followed by an
  inline-asm-driven dynamic alloca, or use a function attribute that
  forces register save via PUSH2. Then a subsequent `__chkstk`-needed
  alloca should be expanded as plain `Sub` rather than `Probe`. Inspect
  MIR via `-stop-after=x86-dyn-alloca-expander`.

Note: this is a "soft" bug — it can cause stack-clash protection to be
bypassed in pathological MIR shapes, but the visible failure surface is
narrow and may require an APX subtarget to trigger. Worth flagging
because the switch is hand-maintained and visibly incomplete vs the
real PUSH/POP opcode universe in X86InstrInfo.td.
