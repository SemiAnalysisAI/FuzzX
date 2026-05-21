## KCFI `kcfi-arity` encodes wrong arity for functions with unused argument registers

`llvm/lib/Target/X86/X86AsmPrinter.cpp:201-231` (`emitKCFITypeId` arity arm).

NON-DEFAULT (requires the `kcfi` *and* `kcfi-arity` module flags, i.e.
`-fsanitize=kcfi -fpatchable-function-entry=0,11 -fno-sanitize-cfi-canonical-jump-tables`
plus the new arity emission).

```cpp
const unsigned ArityToRegMap[8] = {X86::EAX, X86::ECX, X86::EDX, X86::EBX,
                                   X86::ESP, X86::EBP, X86::ESI, X86::EDI};
int Arity;
if (MF.getInfo<X86MachineFunctionInfo>()->getArgumentStackSize() > 0) {
  Arity = 7;
} else {
  Arity = 0;
  for (const auto &LI : MF.getRegInfo().liveins()) {            // <-- *MIR* liveins
    auto Reg = LI.first;
    if (X86::GR8RegClass.contains(Reg) || X86::GR16RegClass.contains(Reg) ||
        X86::GR32RegClass.contains(Reg) ||
        X86::GR64RegClass.contains(Reg)) {
      ++Arity;
    }
  }
}
DestReg = ArityToRegMap[Arity];
```

`getArityFromCallTarget` is intentionally not encoded into the call-site
check (the call side validates only the type hash). The arity is conveyed
out-of-band by the destination register used in the `MOV32ri` of the type
prefix, so a kernel KCFI verifier can decode the register to recover the
arity that the function was declared with.

The bug: **`MF.getRegInfo().liveins()` is the set of *physically live*
argument registers in the lowered MIR, NOT the function's source-level
arity.** When DAGCombine / TwoAddressInstruction / SimpleRegisterCoalescing
prove that an argument is unused (read by nothing), the corresponding
physical register is dropped from the live-in list. The arity therefore
*undercounts* and the prefix register encodes a smaller arity than the IR
signature actually declares.

### Repro (`/tmp/x86bugs/kcfi_arity_unused.ll`)

```llvm
target triple = "x86_64-unknown-linux-gnu"

define dso_local void @f1(i32 %v1, i32 %v2) !kcfi_type !1 {
entry:
  ret void                                  ; both args unused -> dropped from liveins
}

!llvm.module.flags = !{!0, !2}
!0 = !{i32 4, !"kcfi",       i32 1}
!1 = !{i32 199571451}
!2 = !{i32 4, !"kcfi-arity", i32 1}
```

`llc -O2 -mtriple=x86_64-unknown-linux-gnu`:

```
__cfi_f1:
        nop ... nop                            ; padding
        movl    $199571451, %eax              ; <-- %EAX => arity = 0
                                              ; should be %EDX (arity = 2)
```

Compare with a version where the args are consumed (e.g.
`tail call void @use(i32 %v1, i32 %v2)`): the same metadata produces
`movl $199571451, %edx` with the correct arity-2 destination register.
Two functions with the same IR signature `void(i32,i32)` and the same KCFI
hash emit different arity-encoding registers depending only on whether the
optimizer happened to delete the argument uses.

### Why this matters

The whole point of `kcfi-arity` (per the `X86AsmPrinter.cpp` comment block
at 206-213) is to let the runtime verifier *cross-check* the call-site's
expected arity (derived from the call-site's type) against the
landing-site's actual arity (encoded in the prefix register). When the
optimizer removes a tail of unused arguments, the landing-site claims a
smaller arity than the type signature, so:

- A *strict* verifier rejects every otherwise-legal call to such a
  function, breaking the program.
- A *loose* verifier accepts an attacker-forged call from a 0-arity caller
  type into this 2-arity function (because the prefix says "0 args"),
  defeating the arity check entirely.

### Fix sketch

Take the arity from the IR-level function type rather than the MIR liveins.
Roughly:

```cpp
int Arity = 0;
for (const Argument &A : F.args()) {
  if (Arity >= 7) { Arity = 7; break; }
  Type *T = A.getType();
  if (T->isIntegerTy() || T->isPointerTy() || ...) {
    ++Arity;
  } else if (T->isFloatingPointTy() || T->isVectorTy()) {
    /* XMM, not counted */
  } else {
    /* aggregate: assume stack */ Arity = 7; break;
  }
}
if (MF.getInfo<X86MachineFunctionInfo>()->getArgumentStackSize() > 0)
  Arity = 7;
DestReg = ArityToRegMap[Arity];
```

(Or, equivalently, base it on the `CallingConv::SystemV` ABI partitioning
of the function's IR args rather than the MIR live-in survivors.)
