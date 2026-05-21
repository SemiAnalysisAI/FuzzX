# X86DynAllocaExpander: Amount == 0 short-circuit leaks the constant-amount MOV

File: llvm/lib/Target/X86/X86DynAllocaExpander.cpp:207-216 (and contrast with cleanup at 279-285)

```
207  void X86DynAllocaExpander::lower(MachineInstr *MI, Lowering L) {
...
212    int64_t Amount = getDynAllocaAmount(MI, MRI);
213    if (Amount == 0) {
214      MI->eraseFromParent();      // <-- erases the DYN_ALLOCA but NOT
215      return;                     //     the MOV*ri that defines its
216    }                             //     operand-0 amount register.
...
279    Register AmountReg = MI->getOperand(0).getReg();
280    MI->eraseFromParent();
281
282    // Delete the definition of AmountReg.
283    if (MRI->use_empty(AmountReg))
284      if (MachineInstr *AmountDef = MRI->getUniqueVRegDef(AmountReg))
285        AmountDef->eraseFromParent();
```

Reasoning: For all non-zero lowerings the pass deletes the unique MOV defining
the amount vreg (lines 282-285). For the `Amount == 0` fast path, the pseudo is
deleted at line 214 but the defining `MOV32ri 0` / `MOV64ri 0` is left dangling.
Because the pass runs after `NoVRegs` is *not* yet required (it can still hold
vregs that the verifier expects to be alive only if used), a dead `MOV*ri` to a
virtual register that nothing else uses can confuse later assertions. It can
also pessimize codegen by leaving a unused immediate def. Worse, if a later
pass (LiveIntervals/register-allocator interplay) inspects the vreg use after
the pseudo is gone, the asymmetric treatment of the two paths is a latent
correctness hazard.

Repro sketch:
- Build an IR `alloca i8, i32 0, align N` in a frame that requires
  stack-probe (Windows, or function attribute "stack-probe-size"), so the
  DYN_ALLOCA pseudo is emitted and reaches this pass with a constant zero
  amount. Compile with `-mtriple=x86_64-pc-windows-msvc -O0` and inspect the
  MIR after `x86-dyn-alloca-expander` (`-stop-after=x86-dyn-alloca-expander`).
  Expect a dead `MOV64ri 0 -> %vreg` left in the MIR.
