# w635: `AtomicExpandImpl::expandAtomicLoadToCmpXchg` emits invalid IR (vector-typed cmpxchg) for vector atomic loads

## Severity
ICE (verifier abort, "Broken function found, compilation aborted!").

## Source

`llvm/lib/CodeGen/AtomicExpandPass.cpp:668-687`

```cpp
bool AtomicExpandImpl::expandAtomicLoadToCmpXchg(LoadInst *LI) {
  ReplacementIRBuilder Builder(LI, *DL);
  AtomicOrdering Order = LI->getOrdering();
  if (Order == AtomicOrdering::Unordered)
    Order = AtomicOrdering::Monotonic;

  Value *Addr = LI->getPointerOperand();
  Type *Ty = LI->getType();
  Constant *DummyVal = Constant::getNullValue(Ty);          // <-- vector zero

  Value *Pair = Builder.CreateAtomicCmpXchg(
      Addr, DummyVal, DummyVal, LI->getAlign(), Order,      // <-- vector cmpxchg
      AtomicCmpXchgInst::getStrongestFailureOrdering(Order));
  Value *Loaded = Builder.CreateExtractValue(Pair, 0, "loaded");

  LI->replaceAllUsesWith(Loaded);
  LI->eraseFromParent();
  return true;
}
```

`cmpxchg` is restricted by the verifier to integer/pointer operand types
(`llvm/lib/IR/Verifier.cpp` rule: *"cmpxchg operand must have integer or pointer
type"*). `expandAtomicLoadToCmpXchg` makes no attempt to cast the load type to
an integer first.

## Why the existing safety nets miss

`X86TargetLowering::shouldCastAtomicLoadInIR` (`llvm/lib/Target/X86/X86ISelLowering.cpp:33000-33004`):

```cpp
TargetLowering::AtomicExpansionKind
X86TargetLowering::shouldCastAtomicLoadInIR(LoadInst *LI) const {
  if (LI->getType()->getScalarType()->isFloatingPointTy())
    return AtomicExpansionKind::CastToInteger;
  return AtomicExpansionKind::None;
}
```

This *only* triggers when the (scalar) element type is floating point. It
deliberately ignores integer-element vectors. So `load atomic <2 x i64>` /
`<4 x i32>` etc. is **not** cast by `convertAtomicLoadToIntegerType`.

`X86TargetLowering::shouldExpandAtomicLoadInIR`
(`llvm/lib/Target/X86/X86ISelLowering.cpp:32490-32510`) returns `CmpXChg` when
`needsCmpXchgNb(MemType)` is true. `needsCmpXchgNb`
(`llvm/lib/Target/X86/X86ISelLowering.cpp:32458-32467`) returns true for any
128-bit primitive type if `cx16` is available - including vectors. The early
"return None" for 128-bit on x86-64 only fires when *AVX is enabled and the
function does not have `no-implicit-float`*. With `-avx` or `no-implicit-float`,
a 128-bit integer vector falls through to `CmpXChg`.

The result: `expandAtomicLoadToCmpXchg` is called with `LI->getType() ==
<2 x i64>`, builds a `cmpxchg ... <2 x i64>`, and the function verifier rejects
it.

## Repro 1 - `<2 x i64>` with `+cx16,-avx`

```llvm
target triple = "x86_64-unknown-linux-gnu"

define <2 x i64> @load_v2i64(ptr %p) {
  %v = load atomic <2 x i64>, ptr %p seq_cst, align 16
  ret <2 x i64> %v
}
```

```console
$ llc -mtriple=x86_64-unknown-linux-gnu -mattr=+cx16,-avx repro.ll -o /dev/null
cmpxchg operand must have integer or pointer type
 <2 x i64>  %1 = cmpxchg ptr %p, <2 x i64> zeroinitializer, <2 x i64> zeroinitializer seq_cst seq_cst, align 16
in function load_v2i64
LLVM ERROR: Broken function found, compilation aborted!
```

`-stop-after=atomic-expand` shows the malformed IR that the pass emits:

```llvm
define <2 x i64> @load_v2i64(ptr %p) #0 {
  %1 = cmpxchg ptr %p, <2 x i64> zeroinitializer, <2 x i64> zeroinitializer seq_cst seq_cst, align 16
  %loaded = extractvalue { <2 x i64>, i1 } %1, 0
  ret <2 x i64> %loaded
}
attributes #0 = { "target-features"="+cx16,-avx" }
```

## Repro 2 - `<4 x i32>` with `+cx16,-avx`

Same crash:

```llvm
target triple = "x86_64-unknown-linux-gnu"

define <4 x i32> @load_v4i32(ptr %p) {
  %v = load atomic <4 x i32>, ptr %p seq_cst, align 16
  ret <4 x i32> %v
}
```

```console
$ llc -mtriple=x86_64-unknown-linux-gnu -mattr=+cx16,-avx repro.ll -o /dev/null
cmpxchg operand must have integer or pointer type
 <4 x i32>  %1 = cmpxchg ptr %p, <4 x i32> zeroinitializer, <4 x i32> zeroinitializer seq_cst seq_cst, align 16
```

## Repro 3 - `<2 x i64>` with `no-implicit-float`

```llvm
target triple = "x86_64-unknown-linux-gnu"

define <2 x i64> @load_v2i64(ptr %p) #0 {
  %v = load atomic <2 x i64>, ptr %p seq_cst, align 16
  ret <2 x i64> %v
}

attributes #0 = { "no-implicit-float" }
```

```console
$ llc -mtriple=x86_64-unknown-linux-gnu -mattr=+cx16 repro.ll -o /dev/null
cmpxchg operand must have integer or pointer type
 <2 x i64>  %1 = cmpxchg ptr %p, <2 x i64> zeroinitializer, <2 x i64> zeroinitializer seq_cst seq_cst, align 16
```

## Sister code paths (also vector-unsafe)

The same omission exists in two related helpers and could be revisited together:

- `expandAtomicLoadToLL` (`AtomicExpandPass.cpp:652-666`): calls
  `TLI->emitLoadLinked(Builder, LI->getType(), ...)` with the original type.
  Targets advertising LLOnly for a vector load would explode the same way.
- `expandAtomicOpToLLSC` for loads (`AtomicExpandPass.cpp:614-617`): identical
  issue, hidden behind the AtomicExpansionKind::LLSC dispatcher.

These secondary paths are not currently reachable on X86 (no LLSC on X86) so
they don't show up as crashes here, but the same fix shape (bitcast / cast to
integer first) would apply.

## Suggested fix

`expandAtomicLoadToCmpXchg` should normalize the load type to an integer of
matching bitwidth before constructing the cmpxchg, mirroring what
`createCmpXchgInstFun` already does at
`llvm/lib/CodeGen/AtomicExpandPass.cpp:742-764` for the RMW path:

```cpp
Type *OrigTy = LI->getType();
Type *IntTy = OrigTy;
bool NeedBitcast = OrigTy->isFloatingPointTy() || OrigTy->isVectorTy();
if (NeedBitcast)
  IntTy = Builder.getIntNTy(DL->getTypeStoreSizeInBits(OrigTy));
Constant *DummyVal = Constant::getNullValue(IntTy);
Value *Pair = Builder.CreateAtomicCmpXchg(Addr, DummyVal, DummyVal, ...);
Value *Loaded = Builder.CreateExtractValue(Pair, 0, "loaded");
if (NeedBitcast)
  Loaded = Builder.CreateBitCast(Loaded, OrigTy);
```

Alternatively, `shouldCastAtomicLoadInIR` could be widened in
`X86ISelLowering.cpp` to also return `CastToInteger` for vector element types
that aren't natively cmpxchg'able. That at least closes the X86 case but
leaves the generic `expandAtomicLoadToCmpXchg` invariant unstated.

## opt/llc diff summary

- `opt -passes=...`: opt does not run AtomicExpand by default; the bug
  manifests via `llc`.
- `llc` with `-mtriple=x86_64-unknown-linux-gnu -mattr=+cx16,-avx`: aborts with
  *"cmpxchg operand must have integer or pointer type"*.
- `llc` with `-mtriple=x86_64-unknown-linux-gnu` (AVX enabled, no
  `no-implicit-float`): falls through to `AtomicExpansionKind::None`, emits a
  plain `movaps`. So the bug is gated on the target *not* offering a wide
  SSE/AVX scalar load.
