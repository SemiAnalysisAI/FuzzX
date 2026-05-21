# w207: CodeGenPrepare::sinkCmpExpression drops fast-math flags / samesign / metadata

**File:** `llvm/lib/CodeGen/CodeGenPrepare.cpp`
**Lines:** 1874-1945
**Function:** `sinkCmpExpression`

## Summary
When CGP sinks an `icmp`/`fcmp` into each user block to relieve register pressure on the condition register, the freshly-created copy in each user block is built with `CmpInst::Create(opcode, predicate, op0, op1, "")`. This carries the opcode, predicate, and operands, but does NOT copy:
- fast-math flags on `fcmp` (`nnan`, `ninf`, `nsz`, `arcp`, `contract`, `afn`, `reassoc`),
- `samesign` flag on `icmp`,
- range/branch-weight or any other metadata attached to the original `Cmp`.

Only the `DebugLoc` is copied (line 1929). The result is that each sunk `Cmp` is poison-strictly-weaker than the original. Downstream consumers in the user blocks (e.g. SDAG combines, branch folders, DAG isel patterns that rely on `nnan`/`ninf` for select-to-min/max idioms) no longer see those flags and produce worse code or refuse to fire optimizations that the source asserted are valid.

## Source

```c++
if (!InsertedCmp) {
  BasicBlock::iterator InsertPt = UserBB->getFirstInsertionPt();
  assert(InsertPt != UserBB->end());
  InsertedCmp = CmpInst::Create(Cmp->getOpcode(), Cmp->getPredicate(),
                                Cmp->getOperand(0), Cmp->getOperand(1), "");
  InsertedCmp->insertBefore(*UserBB, InsertPt);
  // Propagate the debug info.
  InsertedCmp->setDebugLoc(Cmp->getDebugLoc());
}
```

No `copyIRFlags(Cmp)` or `copyMetadata(*Cmp, ...)` call.

## Reproducer (`test_sinkcmp.ll`)

```llvm
target triple = "x86_64-unknown-linux-gnu"

define i32 @test(float %a, float %b, i32 %x, i32 %y) {
entry:
  %cmp = fcmp nnan ninf olt float %a, %b
  br i1 %cmp, label %if.then, label %if.else

if.then:
  %t = add i32 %x, 1
  %sel.then = select i1 %cmp, i32 %t, i32 %y
  br label %if.end

if.else:
  %sel.else = select i1 %cmp, i32 %x, i32 %y
  br label %if.end

if.end:
  %phi = phi i32 [ %sel.then, %if.then ], [ %sel.else, %if.else ]
  ret i32 %phi
}
```

The fcmp's `nnan ninf` flags are dropped on the sunk copies.

## Reproduce
```
$ /home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/llc \
    -mtriple=x86_64-unknown-linux-gnu -stop-after=codegenprepare \
    test_sinkcmp.ll -o -
```

Observed IR after CGP:
```llvm
if.then:                                          ; preds = %entry
  %0 = fcmp olt float %a, %b           ; lost nnan ninf
  ...
if.else:                                          ; preds = %entry
  %1 = fcmp olt float %a, %b           ; lost nnan ninf
  ...
```

The original `%cmp = fcmp nnan ninf olt` carried `nnan ninf`; the two sunk copies have dropped both.

## Suggested fix
Add `copyIRFlags(Cmp)` (and optionally `copyMetadata(*Cmp)`) after creation:
```c++
InsertedCmp = CmpInst::Create(Cmp->getOpcode(), Cmp->getPredicate(),
                              Cmp->getOperand(0), Cmp->getOperand(1), "");
InsertedCmp->copyIRFlags(Cmp);
InsertedCmp->insertBefore(*UserBB, InsertPt);
InsertedCmp->setDebugLoc(Cmp->getDebugLoc());
```

## Impact
Downstream consumers (DAGCombine select-to-fminmax/fmaxmin, branch optimizations) refuse to fire because the sunk fcmp no longer asserts `nnan`/`ninf`. Same loss for `icmp samesign`.
