# m145: `AMDGPUMCInstLower` `MO_ExternalSymbol` case drops `MO.getTargetFlags()`

*Discovery method: code inspection (during AMDGPUMCInstLower deep audit).*

Sibling defect to the GlobalAddress branch which handles target flags
correctly.

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/AMDGPUMCInstLower.cpp:89-109`:

The `MO_GlobalAddress` branch at lines 89-103 correctly applies
`getSpecifier(MO.getTargetFlags())` when building the
`MCSymbolRefExpr`:

```cpp
case MachineOperand::MO_GlobalAddress: {
  ...
  const MCExpr *Expr = MCSymbolRefExpr::create(
      Sym, getSpecifier(MO.getTargetFlags()), Ctx);
  ...
}
```

The `MO_ExternalSymbol` branch at lines 104-109 does NOT:

```cpp
case MachineOperand::MO_ExternalSymbol: {
  MCSymbol *Sym = Printer.GetExternalSymbolSymbol(MO.getSymbolName());
  const MCExpr *Expr = MCSymbolRefExpr::create(Sym, Ctx);   // <-- no specifier
  MCOp = MCOperand::createExpr(Expr);
  break;
}
```

Any AMDGPU-specific symbol specifier on the operand
(`MO_GOTPCREL`, `MO_REL32_LO`, `MO_REL32_HI`, `MO_ABS32_LO`,
`MO_ABS32_HI`, `MO_REL64`, `MO_ABS64`) is silently dropped.  The
emitted object then carries the wrong relocation type, which can
produce:

* Linker errors (relocation kind mismatch with section type)
* Silent miscompile if the linker treats the unspecified reloc
  as `R_AMDGPU_NONE` and leaves the field unrelocated -> runtime
  jumps to address 0.

## Reachability

The SDAG can emit `ExternalSymbol` operands with target flags via:

* Runtime library calls (`__divdi3`, `__udivdi3`, `__umoddi3`,
  `__floatdidf`, etc.) -- the `RuntimeLibcalls` infrastructure
  routes through `ExternalSymbol`.
* `SI_TCRETURN_CHAIN` (line 233 in this file) re-lowers operand(0)
  which can be an `ExternalSymbol` target -- losing the relocation
  flag produces a wrong tail-call fixup.

## Reproducer

`reduced.ll` (sketch):

```llvm
declare void @external_callee(i64)

define amdgpu_kernel void @t(i64 %x) {
  call void @external_callee(i64 %x)
  ret void
}
```

Build:

```
llc -mtriple=amdgcn -mcpu=gfx950 -O0 -filetype=obj reduced.ll
llvm-readelf -r reduced.o
```

Compare against the same kernel pattern using a defined extern
global instead of a function declaration -- the relocation types
differ even when both should encode `R_AMDGPU_REL32_LO/HI` or
`R_AMDGPU_GOTPCREL32_LO/HI`.

## Suggested fix

Mirror the `GlobalAddress` branch in `MO_ExternalSymbol`:

```cpp
case MachineOperand::MO_ExternalSymbol: {
  MCSymbol *Sym = Printer.GetExternalSymbolSymbol(MO.getSymbolName());
  const MCExpr *Expr = MCSymbolRefExpr::create(
      Sym, getSpecifier(MO.getTargetFlags()), Ctx);    // <-- add specifier
  MCOp = MCOperand::createExpr(Expr);
  break;
}
```

The same defect class exists in the `MO_MCSymbol` branch at lines
113-119 which only handles `MO_FAR_BRANCH_OFFSET`; any other target
flags on MCSymbol operands fall through to `llvm_unreachable` at
line 121 instead of being applied.

## Why the fuzzer hasn't caught it

* The IR fuzzer rarely emits IR that triggers AMDGPU runtime
  libcalls.  Per `MEMORY.md` (Prefer-random-over-idioms), the random
  emitter should include `sdiv i64`/`udiv i64`/`fp-to-i128` patterns
  that lower through RuntimeLibcalls.
* The differential O0-vs-O2 oracle won't catch link-time relocation
  errors -- both opt levels share the same MCInstLower path.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Code path present; ExternalSymbol target flags dropped. |
| ROCm 7.1.1 | Same defect. |
