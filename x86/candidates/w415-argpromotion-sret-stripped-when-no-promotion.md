# ArgumentPromotion strips `sret` -> `noalias` even when no argument is actually promoted

## File and root cause

`llvm/lib/Transforms/IPO/ArgumentPromotion.cpp` -- `promoteArguments`, lines
870-907.

The pointer-arg loop unconditionally rewrites every `sret` parameter into a
`noalias` parameter **before** it asks `findArgParts(...)` whether promotion is
possible:

```c++
for (Argument *PtrArg : PointerArgs) {
    // Replace sret attribute with noalias. This reduces register pressure by
    // avoiding a register copy.
    if (PtrArg->hasStructRetAttr()) {                            // line 873
      unsigned ArgNo = PtrArg->getArgNo();
      F->removeParamAttr(ArgNo, Attribute::StructRet);           // line 875
      F->addParamAttr(ArgNo, Attribute::NoAlias);                // line 876
      for (Use &U : F->uses()) {
        CallBase &CB = cast<CallBase>(*U.getUser());
        CB.removeParamAttr(ArgNo, Attribute::StructRet);         // line 879
        CB.addParamAttr(ArgNo, Attribute::NoAlias);              // line 880
      }
    }

    // If we can promote the pointer to its value.
    SmallVector<OffsetAndArgPart, 4> ArgParts;

    if (findArgParts(PtrArg, DL, AAR, MaxElements, IsRecursive, ArgParts,
                     FAM)) {                                     // line 887
      ...
      ArgsToPromote.insert({PtrArg, std::move(ArgParts)});
    }
  }

  // No promotable pointer arguments.
  if (ArgsToPromote.empty())
    return nullptr;                                              // line 902
```

The `sret` -> `noalias` rewrite is performed before `findArgParts` decides
whether the argument is promotable, and it is also performed before
`return nullptr` at line 902 (no argument promoted). Net effect: a function
that passes through `ArgumentPromotion` without any actual argument being
promoted (because every argument is unpromotable) still loses its `sret`
attribute. The IR is mutated even though `promoteArguments` returns "no
change" semantically.

`sret` carries semantics beyond `noalias`: it tells the backend that this
parameter is the hidden return-value pointer (lowered to a special register on
many ABIs, e.g. `%rax`-return on x86_64 SysV when struct is too large for
register return). Replacing it with bare `noalias` discards the ABI contract.
For an `internal` function with only direct callers in the module this is
self-consistent (all callers were updated), but:

1. The pass mutated IR while returning "nothing promoted", contradicting the
   "argument promotion" name and surprising downstream passes/users.
2. The fix-up loop only touches `F->uses()` -- it implicitly assumes every use
   is a direct call. If a future caller is added through a non-direct path
   (e.g. by another pass holding an `Argument*` reference), it could see
   inconsistent state.
3. The intent of "reduces register pressure" only pays off when promotion
   succeeds; doing the rewrite for unpromotable args yields no benefit but
   still loses the ABI hint.

## Reproducer

`x86/candidates/w415-ap-sret-strip.ll`:

```llvm
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

%S = type { i32, i32 }
declare void @opaque(ptr)

define internal void @callee(ptr sret(%S) %ret) {
  call void @opaque(ptr %ret)           ; opaque escape -> unpromotable
  ret void
}

define void @caller(ptr sret(%S) %ret) {
  call void @callee(ptr sret(%S) %ret)
  ret void
}
```

```
$ opt -passes=argpromotion -S w415-ap-sret-strip.ll
```

After:

```llvm
define internal void @callee(ptr noalias %ret) {        ; sret -> noalias!
  call void @opaque(ptr %ret)
  ret void
}

define void @caller(ptr sret(%S) %ret) {                ; outer caller unchanged
  call void @callee(ptr noalias %ret)                   ; sret -> noalias on call
  ret void
}
```

Note `@callee`'s body is unchanged, no parameter was added or removed -- the
pass would normally be a no-op, but it has mutated the `sret` ABI attribute
to `noalias`.

## Why this is an `IPO/ArgumentPromotion` bug, not an attribute layer issue

`Attribute::StructRet` is intentionally not equivalent to `Attribute::NoAlias`:
`Argument::hasStructRetAttr()` and `getParamStructRetType()` are queried in the
backend (`SelectionDAGBuilder`, `TargetLowering::LowerArguments`) to drive ABI
return-value classification. If a downstream pass or LTO ingest expects to see
`sret` on the function (e.g. to detect "this is the hidden-return slot"), the
information is gone. The transform is also conditional on
`PtrArg->hasStructRetAttr()` but unconditional on whether the pass changes
anything else, which is the structural defect: the mutation lives in the
"intent to promote" loop but always executes.

## Suggested fix

Defer the `sret` -> `noalias` rewrite until after `ArgsToPromote` is populated
**and** known non-empty (or fold it into `doPromotion`, which already runs
only when a promotion will happen). Concretely, move lines 873-882 below the
`if (ArgsToPromote.empty()) return nullptr;` check, and guard with
`if (ArgsToPromote.count(PtrArg))` so only arguments that survive `findArgParts`
get rewritten.
