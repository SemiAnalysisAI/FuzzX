# w46: AsmPrinter::emitGlobalVariable — GOT-equiv skip path runs before TLS handling

File: llvm/lib/CodeGen/AsmPrinter/AsmPrinter.cpp

## Location

`AsmPrinter::emitGlobalVariable` lines 785-812.

```c++
void AsmPrinter::emitGlobalVariable(const GlobalVariable *GV) {
  bool IsEmuTLSVar = TM.useEmulatedTLS() && GV->isThreadLocal();
  ...
  if (IsEmuTLSVar)
    return;

  if (GV->hasInitializer()) {
    if (emitSpecialLLVMGlobal(GV))
      return;

    // Skip the emission of global equivalents. The symbol can be emitted later
    // on by emitGlobalGOTEquivs in case it turns out to be needed.
    if (GlobalGOTEquivs.count(getSymbol(GV)))
      return;
    ...
  }
```

And `isGOTEquivalentCandidate` (lines 2550-2568):

```c++
static bool isGOTEquivalentCandidate(const GlobalVariable *GV, ...) {
  if (!GV->hasGlobalUnnamedAddr() || !GV->hasInitializer() ||
      !GV->isConstant() || !GV->isDiscardableIfUnused() ||
      !isa<GlobalValue>(GV->getOperand(0)))
    return false;
  ...
}
```

## Bug

`isGOTEquivalentCandidate` does **not** exclude `thread_local` globals.
A `private unnamed_addr constant ptr @foo` that happens to be marked
`thread_local` (legal IR; verifier does not forbid TLS on a constant
global with a pointer initializer) will be added to
`AsmPrinter::GlobalGOTEquivs`.

Then in `emitGlobalVariable`:
1. If emulated TLS is on, the function returns at line 793 *before*
   reaching the `GlobalGOTEquivs.count(...)` skip. The GV is never
   emitted. But `emitGlobalGOTEquivs()` (line 2598) later calls
   `emitGlobalVariable(GV)` again — which again hits the EmuTLS early
   return. Net effect: the symbol referenced by other globals' GOT-equiv
   constants is *never defined*, producing an undefined-symbol link
   failure or a relocation against a missing symbol.
2. With non-emulated TLS on ELF (`.tdata`), the GOT-equiv constant
   eventually emitted via `emitGlobalGOTEquivs()` ends up in a non-TLS
   section (because `emitGlobalConstantImpl` will be called from a
   normal context), losing its TLS section placement, or — depending on
   timing — getting emitted twice if a separate code path also emits
   the TLS variable.

## Pattern match

"AsmPrinter handling of global GOT-relative symbol with TLS" from the
bug-pattern list.

## Reproducibility

Source-level finding. A direct repro requires IR with a TLS
`@__got_equivalent` style global whose initializer is `ptr @other_gv`,
plus another global initializer that references it via the
`sub (i64 ptrtoint(@equiv to i64), i64 ptrtoint(@base to i64))`
PC-relative idiom that `supportIndirectSymViaGOTPCRel` recognises
(Darwin or ELF with the right TLOF flag). Has not been bisected against
clang's IR emitters; flagged for IR fuzzer follow-up because the
omission of any TLS check in `isGOTEquivalentCandidate` is structural.

## Fix sketch

Add `!GV->isThreadLocal()` to the `isGOTEquivalentCandidate` early-out,
or short-circuit in `computeGlobalGOTEquivs` before inserting into
`GlobalGOTEquivs`.
