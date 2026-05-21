# NewGVN CSEs readonly calls across mismatched operand bundles

## File and root cause

`llvm/lib/Transforms/Scalar/NewGVN.cpp` — `createCallExpression`
(line 1305) and `CallExpression::equals` (line 941).

`createCallExpression` has the comment:

```
  // FIXME: Add operand bundles for calls.
```

It only calls `setBasicExpressionInfo`, which copies `I->op_begin()..op_end()`
and the opcode. Operand-bundle metadata (bundle tags, bundle structure, the
presence/absence of bundles) is not part of the BasicExpression/CallExpression
identity. `CallExpression::equals` only intersects attributes:

```
bool CallExpression::equals(const Expression &Other) const {
  if (!MemoryExpression::equals(Other)) return false;
  if (auto *RHS = dyn_cast<CallExpression>(&Other))
    return Call->getAttributes()
        .intersectWith(Call->getContext(), RHS->Call->getAttributes())
        .has_value();
  return false;
}
```

Two readonly calls in the same `MemoryLeader` class with identical regular
operands but different (or only one) operand bundles end up in the same
CongruenceClass.

LLVM's LangRef states that the presence of an operand bundle prevents the call
from being CSE'd / hoisted in general — `"deopt"` bundles attach a
deoptimization site that the runtime relies on; eliminating the second call
deletes a deopt point. Removing a `"funclet"`, `"ptrauth"`, `"clang.arc.*"`,
`"kcfi"`, or `"gc-live"` bundle similarly changes semantics.

## IR repro (minimal)

```llvm
declare i32 @rd(i32) memory(read)

define i32 @test(i32 %x) {
  %a = call i32 @rd(i32 %x)
  %b = call i32 @rd(i32 %x) [ "deopt"() ]
  %r = add i32 %a, %b
  ret i32 %r
}
```

## opt diff (newgvn deletes the bundle-bearing call)

```
$ opt -passes=newgvn -S bundle.ll
define i32 @test(i32 %x) {
  %a = call i32 @rd(i32 %x)
  %r = add i32 %a, %a            ; <- both %a and %b RAUW'd to %a
  ret i32 %r
}
```

For comparison, the legacy `-passes=gvn` preserves both calls (does not CSE
across differing bundle state).

## llc diff (observable)

```
$ llc -mtriple=x86_64-linux-gnu  (without newgvn)
    callq rd@PLT       ; %a
    callq rd@PLT       ; %b with deopt bundle
    addl  %ebp, %eax

$ opt -passes=newgvn | llc -mtriple=x86_64-linux-gnu
    callq rd@PLT
    addl  %eax, %eax   ; one call gone; deopt site silently removed
```

## Why it's a miscompile, not a missed-opt

The result of `%a + %b` may numerically be the same as `2*%a` if `@rd` is
truly pure. But:

* Removing the `deopt()` bundle drops a deoptimization site. The runtime
  (e.g., a JIT) may have side tables keyed on that call site; the program
  becomes unable to deoptimize there. This is a semantic change, not just an
  optimization.
* The same logic applies to `"ptrauth"`, `"funclet"`, `"kcfi"`, `"clang.arc.*"`
  bundles — any tag that conveys ABI/security/runtime contract.
* GVN (the legacy pass) correctly refuses to CSE across mismatched bundles
  for this reason; NewGVN ignores them entirely.

## Suggested fix

Include the operand-bundle "signature" (tag IDs and number of bundle ops, at
minimum) in `CallExpression`'s hash/equality. The simplest safe fix is to
return `nullptr` (or refuse to CSE) when either call has an operand bundle of
a tag listed in `CallBase::bundleOperandHasAttr` semantics tags
(`deopt`, `funclet`, `ptrauth`, `kcfi`, `clang.arc.attachedcall`, `gc-live`,
`gc-transition`, ...). The FIXME at line 1307 already acknowledges this.
