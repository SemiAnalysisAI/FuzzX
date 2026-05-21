# MachineCopyPropagation forwardUses misses implicit-DEF of copy source on the user

File: llvm/lib/CodeGen/MachineCopyPropagation.cpp
- `hasImplicitOverlap` (lines 768-776)
- `forwardUses` (lines 816-914), in particular the special-case check at
  lines 881-886 that ONLY fires when the user MI is itself a COPY.

## Pattern

`forwardUses` rewrites a USE of CopyDst in MI to a use of CopySrc (the
forwarded register). Before rewriting it calls:

```cpp
if (hasImplicitOverlap(MI, MOUse))
  continue;
```

`hasImplicitOverlap` iterates `MI.uses()` looking for **implicit USE**
operands that overlap with the register being replaced (MOUse). The loop
explicitly requires `MIUse.isImplicit() && MIUse.isUse()`. Implicit-DEFs are
not considered.

Then there is a follow-up safety check at lines 881-886:

```cpp
if (isCopyInstr(MI, *TII, UseCopyInstr) &&
    MI.modifiesRegister(CopySrc, TRI) &&
    !MI.definesRegister(CopySrc, /*TRI=*/nullptr)) {
  // bail
}
```

This check is **gated on `isCopyInstr(MI)`**, i.e. only when the user MI is
itself a COPY. A non-copy user MI that implicit-defs CopySrc bypasses both
checks.

## When can this miscompile

After rewriting MOUse to CopySrc, the user MI's operand list now contains an
**explicit use** of CopySrc plus an **implicit def** of CopySrc. For a typical
x86 RMW instruction (XCHG, CMPXCHG, INC/DEC of GPR, MUL/DIV's RDX:RAX side
effect, RDTSCP defining $rcx, etc.) the implicit def fires alongside the
explicit-use as a single atomic semantic step, so the value read is still the
prior value — semantically equivalent to the pre-forward state when the COPY
established CopySrc == CopyDst at this program point.

That equivalence breaks only if some prior instruction has separately mutated
CopySrc between the COPY and MI without going through the tracker
invalidation. The forward tracker invalidates correctly on plain defs and
regmasks, so in practice the rewrite is safe for normal x86 codegen.

The reason this is still worth filing:

1. The defensive comment that motivates lines 881-886 — "instruction is not a
   copy that partially overwrites the original copy source" — applies to
   non-COPY instructions too (a `BLSR64rr $rbx, $rbx, implicit-def $eflags`
   for instance writes the destination GR, but the same logic about "MI
   modifies CopySrc" should bail whenever MI re-defs CopySrc and that re-def
   isn't the operand we're forwarding into).
2. With the AMDGPU example called out by the file header (a V_MOVRELS uses
   an implicit tied super-register), the analogous AMDGPU-style "implicit-def
   of source as tied write" pattern would NOT be caught: e.g.
   `MI uses %xmm0, implicit-def %ymm0` after the explicit operand is the same
   sub-register as part of the wider implicit def.
3. The gating on `isCopyInstr(MI)` at line 881 is itself fragile: a
   target-specific peephole that simplifies the user MI into a COPY in
   `TII->simplifyInstruction(MI)` (called at line 963) runs AFTER forwardUses,
   so the gate at 881 examines MI as it was BEFORE the simplifier — meaning a
   pre-simplification non-copy that later canonicalises into a COPY can have
   already been forwarded under the weaker check.

## Why I'm not filing as a confirmed miscompile

I have not constructed an MIR input that demonstrates a wrong-code result on
x86 today. The atomic-instruction-execution property of x86 RMW ops makes the
naive `implicit-def of CopySrc` pattern semantically equivalent in the cases
I traced. The concern is the gap between the documented intent (per the
lines 879-880 comment, "tracker mechanism cannot cope with that") and the
narrowness of the gate.

## Suggested tightening

Replace the `isCopyInstr(MI)` gate with a generic check:

```cpp
if (MI.modifiesRegister(CopySrc, TRI) && !MI.definesRegister(CopySrc, nullptr))
  continue;
```

(i.e., bail any time the user MI redefines CopySrc through means other than
the operand we're forwarding into). Or, extend `hasImplicitOverlap` to consider
implicit-DEFs as well as implicit-USEs.

## Confidence

Low-to-medium. Structural gap is real; concrete miscompile not demonstrated.
Filed for fuzzer attention because the closely-related bug class
(`MachineCopyPropagation that propagates across an instruction with
implicit-def of the source`) is in the worker brief.
