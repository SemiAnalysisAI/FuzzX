# w40: X86AsmPrinter::emitEndOfAsmFile COFF `_fltused` early-return skips `__morestack_addr`

File: `llvm/lib/Target/X86/X86AsmPrinter.cpp:1051-1137`

## Pattern

Wrong handling for an unusual triple (x86_64-windows-msvc, large code model, with __morestack).

## Source

```cpp
void X86AsmPrinter::emitEndOfAsmFile(Module &M) {
  const Triple &TT = TM.getTargetTriple();

  if (TT.isOSBinFormatMachO()) {
    ...
  } else if (TT.isOSBinFormatCOFF()) {
    // ImportCallOptimization block (~1071-1096) ...

    if (usesMSVCFloatingPoint(TT, M)) {
      ...
      MCSymbol *S = MMI->getContext().getOrCreateSymbol(SymbolName);
      OutStreamer->emitSymbolAttribute(S, MCSA_Global);
      return;                                 // <-- early return
    }
  } else if (TT.isOSBinFormatELF()) {
    FM.serializeToFaultMapSection();
  }

  // Emit __morestack address if needed for indirect calls.
  if (TT.isX86_64() && TM.getCodeModel() == CodeModel::Large) {
    if (MCSymbol *AddrSymbol = OutContext.lookupSymbol("__morestack_addr")) {
      ...
      OutStreamer->emitSymbolValue(GetExternalSymbolSymbol("__morestack"), PtrSize);
    }
  }
}
```

## Bug

On a COFF target, when `usesMSVCFloatingPoint(TT, M)` is true and `_fltused`
is emitted, the function `return`s before reaching the `__morestack_addr`
emission at the bottom of the function. So an x86_64-windows-msvc target
built with `-mcmodel=large` and indirect calls to `__morestack` (segmented
stacks) will silently drop the `__morestack_addr` constant the relocation
expects. Linker error or wrong call destination.

The early `return` should be replaced with a `break` from the `else if`
chain — equivalent to having `if (usesMSVCFloatingPoint(...)) { ... emit
symbol attribute ... }` without the `return`. The Mach-O branch doesn't
return early because its `emitSubsectionsViaSymbols` is the trailing call
that needs to come after everything; here the COFF MSVC-float emission is
not order-critical and should not skip the trailing `__morestack_addr`
logic.

## Severity

Very narrow trigger (Windows x86_64 + large code model + segmented stacks +
any FP use). Realistically rare but a clear control-flow escape — the
generated object will fail to link, the user will get an undefined symbol
to `__morestack_addr`.

## Confirmed Repro

`llc -mtriple=x86_64-pc-windows-msvc -code-model=large -O0`

```ll
target triple = "x86_64-pc-windows-msvc"
declare void @callee()
define void @foo() #0 {
  call void @callee()
  %x = fadd float 1.0, 2.0   ; triggers _fltused
  ret void
}
attributes #0 = { "split-stack" }
```

With `fadd`: asm contains `callq *__morestack_addr(%rip)` and `.globl
_fltused`, but **NO `__morestack_addr:` symbol definition**. Linker fails
with undefined symbol.

Without `fadd`: asm correctly contains:
```
	.section	.rdata,"dr"
__morestack_addr:
	.quad	__morestack
```

## Status

asm-confirmed.
