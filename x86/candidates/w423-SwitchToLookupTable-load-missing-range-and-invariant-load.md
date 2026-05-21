# w423 — `SwitchToLookupTable` builds `switch.load` without `!range` or `!invariant.load` despite the table being a private constant of known-bounded entries

Severity: missed optimization. Not a metadata-drop (the original IR had no
load to carry metadata) — the issue is that the *newly created* load fails to
attach metadata that is statically derivable from the table contents.

## Where

`llvm/lib/Transforms/Utils/SimplifyCFG.cpp:6968-6993`
(method `SwitchReplacement::replaceSwitch`, `LookupTableKind` arm):

```cpp
case LookupTableKind: {
  ++NumLookupTables;
  auto *Table =
      new GlobalVariable(*Func->getParent(), Initializer->getType(),
                         /*isConstant=*/true, GlobalVariable::PrivateLinkage,
                         Initializer, "switch.table." + Func->getName());
  Table->setUnnamedAddr(GlobalValue::UnnamedAddr::Global);
  Table->setAlignment(DL.getPrefTypeAlign(ValueType));
  Type *IndexTy = DL.getIndexType(Table->getType());
  auto *ArrayTy = cast<ArrayType>(Table->getValueType());

  if (Index->getType() != IndexTy) {
    unsigned OldBitWidth = Index->getType()->getIntegerBitWidth();
    Index = Builder.CreateZExtOrTrunc(Index, IndexTy);
    if (auto *Zext = dyn_cast<ZExtInst>(Index))
      Zext->setNonNeg(
          isUIntN(OldBitWidth - 1, ArrayTy->getNumElements() - 1));
  }

  Value *GEPIndices[] = {ConstantInt::get(IndexTy, 0), Index};
  Value *GEP =
      Builder.CreateInBoundsGEP(ArrayTy, Table, GEPIndices, "switch.gep");
  return Builder.CreateLoad(ArrayTy->getElementType(), GEP, "switch.load");
}
```

## What's missing

The lookup table is a `private constant unnamed_addr` global whose contents
are entirely known at compile time. The load at line 6992 should therefore
have, but does not get:

1. **`!invariant.load`** — the global is `isConstant=true`, so the loaded
   value cannot change. Marking the load invariant unlocks GVN/CSE
   optimizations across calls.
2. **`!range`** — since all entries in the table are statically known
   `ConstantInt`s, the union of their values is a `ConstantRange` that can be
   attached to the load. Right now, downstream passes have to re-derive the
   range from scratch (and often don't — most passes don't reason through
   `getelementptr` of a constant array of constants to infer range).
3. **`!nonnull`** (when applicable) — if the value type is a pointer and all
   table entries are non-null globals, the load should carry `!nonnull`. The
   current code doesn't add this either.

For integer payloads the values often go down the `BitMapKind` or
`LinearMapKind` paths instead (which avoid loads altogether), so this only
bites for non-linear integer/pointer tables.

## Reproducer

`/tmp/w420/t41_lookup_load.ll` — minimal three-case switch producing a
lookup table:

```ll
target datalayout = "e-m:e-p:32:32-i64:64-v128:64:128-a:0:32-n32-S64"
target triple = "armv7a--none-eabi"

define i32 @test1(i32 %n) {
entry:
  switch i32 %n, label %sw.default [
  i32 0, label %sw.bb
  i32 1, label %sw.bb1
  i32 2, label %sw.bb2
  ]
sw.bb:    br label %return
sw.bb1:   br label %return
sw.bb2:   br label %return
sw.default: br label %return
return:
  %retval.0 = phi i32 [ 15498, %sw.default ], [ 15532, %sw.bb2 ],
                      [ 5678, %sw.bb1 ], [ 1234, %sw.bb ]
  ret i32 %retval.0
}
```

The arm triple is used only because the host-machine TTI on x86 is permissive
and the lookup-table fold prefers the bitmap form for narrow integers; on arm
the table form actually fires. The bug is in target-independent code in
`SimplifyCFG.cpp`. Pass-spec: `opt -passes='simplifycfg<switch-to-lookup>'`
(the `switch-to-lookup` qualifier is necessary to enable the late lookup-
table fold; it is set in the O2 pipeline at
`PassBuilderPipelines.cpp:1416`).

After:

```ll
@switch.table.test1 = private unnamed_addr constant [3 x i32]
    [i32 1234, i32 5678, i32 15532], align 4

define i32 @test1(i32 %n) {
entry:
  %0 = icmp ult i32 %n, 3
  br i1 %0, label %switch.lookup, label %return

switch.lookup:
  %switch.gep = getelementptr inbounds [3 x i32], ptr @switch.table.test1, i32 0, i32 %n
  %switch.load = load i32, ptr %switch.gep, align 4     ; no !range, no !invariant.load
  br label %return
return:
  %retval.0 = phi i32 [ %switch.load, %switch.lookup ], [ 15498, %entry ]
  ret i32 %retval.0
}
```

Could trivially be:

```
  %switch.load = load i32, ptr %switch.gep, align 4,
      !invariant.load !{}, !range !{i32 1234, i32 15533}
```

## Notes

- Suggested fix: in `SwitchReplacement::buildTable` (the LookupTableKind path
  around `SimplifyCFG.cpp:6917-6921`) record min/max of the integer table
  entries, then in `replaceSwitch` add `!invariant.load` unconditionally and
  `!range` (or `!nonnull` for pointers) computed from the recorded bounds.
- The bitmap and linear-map paths already encode the bounds *in their
  arithmetic*, so they don't need this fix.
- This is a long-standing oversight — same `Builder.CreateLoad` form has been
  in the file across many LLVM versions.
