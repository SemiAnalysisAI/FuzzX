# m098: `AMDGPUUnifyDivergentExitNodes` misses the verifier-permitted bitcast between `musttail` call and `ret`

*Discovery method: code inspection.*

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/AMDGPUUnifyDivergentExitNodes.cpp:256-261`:

```cpp
if (auto *RI = dyn_cast<ReturnInst>(Term)) {
  auto *CI = dyn_cast_or_null<CallInst>(RI->getPrevNode());
  if (CI && CI->isMustTailCall())
    continue;
  ...
}
```

The pass's musttail-skip check examines only `RI->getPrevNode()`.  But
the LLVM IR Verifier (`Verifier.cpp:4283-4290`) explicitly permits an
**optional bitcast** between a `musttail` call and the `ret` it
precedes:

```
musttail call must precede a ret with an optional bitcast
```

When that bitcast is present, `RI->getPrevNode()` returns the
`BitCastInst`, `dyn_cast_or_null<CallInst>` returns nullptr, the block
is NOT skipped, and the `ret` is replaced with an `UncondBr` to the
unified-return block.  This destroys the musttail invariant
(`musttail` requires the `ret` to be the immediate next instruction
modulo the optional bitcast) and produces broken IR.

## Reproducer

`reduced.ll`:

```llvm
target triple = "amdgcn-amd-amdhsa"

declare ptr @callee_ptr(i32)

define ptr @fuzz(i32 %tid) {
entry:
  %div = icmp slt i32 %tid, 0
  br i1 %div, label %tail, label %normal

normal:
  ret ptr null

tail:
  %r1  = musttail call ptr @callee_ptr(i32 %tid)
  %r1c = bitcast ptr %r1 to ptr
  ret ptr %r1c
}
```

Run:

```bash
/opt/rocm-7.1.1/lib/llvm/bin/opt -S \
    -passes=amdgpu-unify-divergent-exit-nodes \
    -mtriple=amdgcn-amd-amdhsa -mcpu=gfx950 reduced.ll
```

Result:

```text
musttail call must precede a ret with an optional bitcast
  %r1 = musttail call ptr @callee_ptr(i32 %tid)
LLVM ERROR: Broken module found, compilation aborted!
```

With `-disable-verify` the broken IR slips through:

```llvm
tail:                                             ; preds = %entry
  %r1  = musttail call ptr @callee_ptr(i32 %tid)
  %r1c = bitcast ptr %r1 to ptr
  br label %UnifiedReturnBlock                    ; <-- musttail invariant broken

UnifiedReturnBlock:
  %UnifiedRetVal = phi ptr [ %r1c, %tail ], [ null, %entry ]
  ret ptr %UnifiedRetVal
```

The `br` after a `musttail` call (rather than `ret`) means the tail
call is no longer at the function's exit, so the musttail-guaranteed
tail-call semantics (no stack growth, ABI-compatible reuse of the
caller's slot) are destroyed -- the codegen has to emit a regular
call instead, with whatever ABI consequences that brings.

The companion test `test/CodeGen/AMDGPU/do-not-unify-divergent-exit-nodes-with-musttail.ll`
covers only the bitcast-LESS form, missing this case entirely.

## How a fix should look

Mirror the verifier's walk past the optional bitcast:

```cpp
Instruction *Prev = RI->getPrevNode();
if (auto *BC = dyn_cast_or_null<BitCastInst>(Prev))
  Prev = BC->getPrevNode();
auto *CI = dyn_cast_or_null<CallInst>(Prev);
if (CI && CI->isMustTailCall())
  continue;
```

## Why this matters in the default pipeline

`AMDGPUUnifyDivergentExitNodes` is registered in
`AMDGPUPassConfig::addCodeGenPrepare` (`AMDGPUTargetMachine.cpp`),
unconditionally on every opt level for the SDAG path.  Any source-level
language that emits `musttail call` with a covariant-return bitcast
(C++ override returns, opaque-pointer migrations that leave a
trivial bitcast) on AMDGPU hits this whenever the kernel has another
divergent exit.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Reproduces (verifier aborts; `-disable-verify` produces broken IR). |
| ROCm 7.1.1 (`/opt/rocm-7.1.1/lib/llvm/bin/opt`) | Reproduces. |
| ROCm 7.2.3 (source build) | Reproduces. |
