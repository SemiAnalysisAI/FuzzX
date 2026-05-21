## RS4GC crashes on vector `addrspacecast` (NDEBUG-elided assert -> bad cast)

`llvm/lib/Transforms/Scalar/RewriteStatepointsForGC.cpp:351-443` (`findBaseDefiningValueOfVector`)

```cpp
static Value *findBaseDefiningValueOfVector(Value *I, DefiningValueMapTy &Cache,
                                            IsKnownBaseMapTy &KnownBases) {
  // ... handlers for Argument, Constant, LoadInst, InsertElementInst,
  // ShuffleVectorInst, GetElementPtrInst, FreezeInst, BitCastInst,
  // CallInst/InvokeInst ...

  // A PHI or Select is a base defining value.  The outer findBasePointer
  // algorithm is responsible for constructing a base value for this BDV.
  assert((isa<SelectInst>(I) || isa<PHINode>(I)) &&
         "unknown vector instruction - no base found for vector element");
  Cache[I] = I;
  setKnownBase(I, /* IsKnownBase */false, KnownBases);
  return I;
}
```

The vector form of the BDV finder does NOT handle `AddrSpaceCastInst`. The
scalar `findBaseDefiningValue` (line 496-510) at least *has* a `CastInst`
arm whose `stripPointerCasts` plus assertion fires for an addrspacecast (see
sibling bug w471). The vector form does not even take that path: `BitCastInst`
is matched explicitly at line 421-425, and every other cast (including
`AddrSpaceCastInst`) hits the final `assert((SelectInst || PHINode))`.

In an `NDEBUG` build (this fuzzer build is `Optimized`) the assert is elided
and execution falls through to lines 440-442. That is harmless by itself,
but the *caller* expects the returned BDV to be valid for downstream queries.
The next consumer in `findBasePointer` then casts the BDV to `PHINode` /
`SelectInst` / `ExtractElementInst` / `InsertElementInst` / `ShuffleVectorInst`
via `visitBDVOperands`'s `llvm_unreachable("unexpected BDV type")` (line 882)
or via the cloning loop at line 1116 (`I->clone()` followed by `setOperand`).
In any of these paths the address-space-cast instruction is treated as one
of the listed merge/extract opcodes; the misclassification produces either
a segfault or a wild `setOperand` against a CastInst whose opcode-specific
operand layout does not match.

### Candidate IR

```
target triple = "x86_64-unknown-linux-gnu"

declare void @bar()
declare token @llvm.experimental.gc.statepoint.p0(i64, i32, ptr, i32, i32, ...)

define <2 x ptr addrspace(1)> @test(<2 x ptr> %p) gc "statepoint-example" {
  %p1 = addrspacecast <2 x ptr> %p to <2 x ptr addrspace(1)>
  call void @bar()
  ret <2 x ptr addrspace(1)> %p1
}
```

### Observed (wrong) output

`opt -passes=rewrite-statepoints-for-gc -disable-verify -S`:

```
PLEASE submit a bug report to https://github.com/llvm/llvm-project/issues/
Stack dump:
0.  Program arguments: opt -passes=rewrite-statepoints-for-gc -disable-verify -S
1.  Running pass "rewrite-statepoints-for-gc" on module "..."

  ... llvm::sys::PrintStackTrace ...
  ... <unknown frames inside RewriteStatepointsForGC::runOnFunction> ...
  llvm::RewriteStatepointsForGC::runOnFunction(llvm::Function&, ...) + 3981
```

The crash is reached from `runOnFunction`'s call chain into the BDV walker.
Identical signature in `t_vec_ascast2.ll` (vector arg + extractelement after
a statepoint) reproduces the same backtrace.

### Expected behavior

In an assertion build the assert at line 438 fires with
"unknown vector instruction - no base found for vector element". Both
builds need an explicit `AddrSpaceCastInst` arm; the scalar finder has
the same gap (sibling bug w471) but at least asserts on
"unsupported addrspacecast" if `stripPointerCasts` changes the
address space. The vector path has neither defense.

Two possible fixes (mirroring the scalar code):

1. Treat a vector `addrspacecast` whose source address space is non-GC as
   its own base (analogous to the scalar `IntToPtrInst` arm at line 490-494
   and the vector `LoadInst` arm at line 376-380).
2. Reject the case explicitly with a clear diagnostic.

### Reproducers

* `/tmp/rs4gc_test/t_vec_ascast.ll`
* `/tmp/rs4gc_test/t_vec_ascast2.ll`
