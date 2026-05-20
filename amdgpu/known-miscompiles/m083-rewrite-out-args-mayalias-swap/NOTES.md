# m083: `AMDGPURewriteOutArguments` swaps return values for may-alias out arguments

*Discovery method: code inspection.*

## The bug

`AMDGPURewriteOutArguments.cpp:252-288` finds the "store to clean up"
for each candidate out-arg by issuing a single MemoryDependence query
from the bottom of the return block:

```cpp
MemDepResult Q = MDA->getPointerDependencyFrom(
    MemoryLocation::getBeforeOrAfter(OutArg), true, BB->end(), BB, RI);
StoreInst *SI = nullptr;
if (Q.isDef())
  SI = dyn_cast<StoreInst>(Q.getInst());

if (SI) {
  ReplaceableStores.emplace_back(RI, SI);   // <-- no check SI's pointer == OutArg
}
```

The pass then unconditionally erases that store and uses its value
operand as the OutArg's return-struct field, *without verifying that
`SI->getPointerOperand()` is `OutArg`*.

`MemoryDependenceAnalysis::getPointerDependencyFrom` stops at the
first store that **may-alias** the query pointer, ignoring pointer
identity.  When two pointer args are not `noalias`, MDA returns the
*last* store in the block as the def for *both* OutArgs.  After the
do-while loop processes both candidates, each OutArg has been paired
with the OTHER store's value, producing a clean swap.

The dev comment at lines 226-233 acknowledges that MDA returns "the
second store but not the first" on the first pass and expects the
retry loop to fix it.  It does not: the retry just hits the symmetric
problem for the other OutArg.

## Reproducer

`reduced.ll`:

```llvm
target triple = "amdgcn-amd-amdhsa"

define void @foo(ptr addrspace(5) %out0, ptr addrspace(5) %out1) {
entry:
  store i32 1, ptr addrspace(5) %out0, align 4
  store i32 2, ptr addrspace(5) %out1, align 4
  ret void
}
```

Run:

```bash
/opt/rocm-7.1.1/lib/llvm/bin/opt -S -amdgpu-rewrite-out-arguments \
    -mtriple=amdgcn-amd-amdhsa -mcpu=gfx950 \
    amdgpu/known-miscompiles/m083-rewrite-out-args-mayalias-swap/reduced.ll
```

Result:

```llvm
define private %foo @foo.body(ptr addrspace(5) %out0, ptr addrspace(5) %out1) #0 {
entry:
  ret %foo { i32 2, i32 1 }                ; <-- swapped values
}

define void @foo(ptr addrspace(5) %0, ptr addrspace(5) %1) #1 {
  %3 = call %foo @foo.body(ptr addrspace(5) poison, ptr addrspace(5) poison)
  %4 = extractvalue %foo %3, 0             ; gets 2
  store i32 %4, ptr addrspace(5) %0, align 4  ; *%out0 = 2 (was 1)
  %5 = extractvalue %foo %3, 1             ; gets 1
  store i32 %5, ptr addrspace(5) %1, align 4  ; *%out1 = 1 (was 2)
  ret void
}
```

Original semantics: `*%out0 = 1, *%out1 = 2`.
After rewrite:     `*%out0 = 2, *%out1 = 1`.

Reproduces on both ROCm 7.1.1 and the in-tree LLVM HEAD build.

## Why the existing test suite misses this

`llvm/test/CodeGen/AMDGPU/rewrite-out-arguments.ll:169-173`
(`@multiple_same_return_mayalias`) is exactly this broken input, and
the CHECK lines at 672-684 of the same test file encode the buggy
`{ i32 2, i32 1 }` output as the expected result.  The bug is baked
into the test oracle.

## Caveat: pass is not in the default pipeline

`AMDGPURewriteOutArguments` is registered with `INITIALIZE_PASS` but
never added to `AMDGPUTargetMachine`'s codegen pipeline.  It is
reachable only through `opt -amdgpu-rewrite-out-arguments` or custom
tooling.  `run_ll_reproducer.sh` cannot trigger it, so this report
uses `opt` directly (same caveat as m082).

## How a fix should look

Validate that the MDA-returned store is the one we expect:

```cpp
if (SI && getUnderlyingObject(SI->getPointerOperand()) == OutArg) {
  ReplaceableStores.emplace_back(RI, SI);
} else {
  ThisReplaceable = false;
  break;
}
```

(or strip bitcasts and compare exactly).  Bailing out conservatively
when MDA returns a store whose pointer is *not* the OutArg also lets
the existing `noalias`-only test still pass, and forces the
may-alias test to be marked as a negative.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Reproduces: body returns `{ 2, 1 }`. |
| ROCm 7.1.1 (`/opt/rocm-7.1.1/lib/llvm/bin/opt`) | Reproduces: body returns `{ 2, 1 }`. |
