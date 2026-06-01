# 019 — Internal (local-linkage) ptx_kernel with a byval parameter: assert(!isKernelFunction) fires; in release the kernel's byval param is silently over-aligned to >= 16

- **Kind:** crash (assert) / miscompile in release
- **Reachable via:** default llc
- **Component:** 1430 NVPTXUtilities.cpp:44-52  (round-3 area `T20-forwardparams-setbyval-markptrs`)
- **Candidate id:** r3_04

## Summary

an `internal` ptx_kernel with a byval param trips `assert(!isKernelFunction)`; release over-aligns the host-filled `.param` slot

## Mechanism / root cause

getFunctionParamOptimizedAlign() decides whether a function's params may be over-aligned beyond ABI based solely on linkage:

  if (!F || !F->hasLocalLinkage() || F->hasAddressTaken(...)) return ABITypeAlign;
  assert(!isKernelFunction(*F) && "Expect kernels to have non-local linkage");
  return std::max(Align(16), ABITypeAlign);

The over-align branch (max(16, ABIalign)) is justified only because 'the compiler controls all call sites' of local-linkage functions. The assert encodes the invariant that kernels are never local-linkage, since a kernel's byval params are filled by the *host*, which the compiler does NOT control. But `internal`/`private` is local linkage, and an internal ptx_kernel is accepted by the IR verifier. Such a kernel takes the local-linkage path, the gate `!hasLocalLinkage()` is false, and execution reaches the assert -> abort in a debug build.

In a release (NDEBUG) build the assert is compiled out and the function returns max(16, ABIalign) for the internal kernel. NVPTXSetByValParamAlign (line 67) then bumps the kernel's byval param Alignment attribute, and NVPTXAsmPrinter (IsKernelFunc path, line 1430 -> GetOptimalAlignForParam -> getFunctionParamOptimizedAlign, line 1414) emits `.param .align 16 .b8 ...` instead of the natural ABI alignment. The kernel parameter buffer is laid out by the host/driver from natural ABI alignment; raising the device-side `.param` alignment (and the vectorizable byval load alignment) above what the host provides can misalign param-bank offsets / vector loads -> wrong reads at runtime (release miscompile). The same value is also fed to byval load-alignment propagation in NVPTXSetByValParamAlign.cpp:144 (propagateAlignmentToLoads), marking loads as more-aligned than the actual data.

## Trigger

An `internal` (or `private`) function with `ptx_kernel` calling convention (or kernel metadata) that has a `byval` parameter. Valid IR: the verifier accepts it.

## Reproducer

```
target triple = "nvptx64-nvidia-cuda"

define internal ptx_kernel void @ik(ptr byval(i32) %p) {
  ret void
}
```

Command: `llc -mtriple=nvptx64 -o - repro.ll`

## Observed (wrong) output

```
Assertion failed: (!isKernelFunction(*F) && "Expect kernels to have non-local linkage"), function getFunctionParamOptimizedAlign, file NVPTXUtilities.cpp, line 51.
PLEASE submit a bug report to https://github.com/llvm/llvm-project/issues/ and include the crash backtrace and instructions to reproduce the bug.
Stack dump:
0.	Program arguments: /Users/justinlebar/code/llvm2/build/bin/llc -mtriple=nvptx64 /Users/justinlebar/code/FuzzX/NVPTX/scratch/r3_04.ll -o -
1.	Running pass 'Function Pass Manager' on module '/Users/justinlebar/code/FuzzX/NVPTX/scratch/r3_04.ll'.
2.	Running pass 'Set alignment of byval parameters (NVPTX)' on function '@ik'
 #7 llvm::getFunctionArgumentAlignment(llvm::Function const*, llvm::Type*, unsigned int, llvm::DataLayout const&)
 #8 setByValParamAlignment(llvm::Function&)
 #9 llvm::FPPassManager::runOnFunction(llvm::Function&)
(llc exit code: 134 = SIGABRT)
```

## Expected

llc should successfully compile the internal ptx_kernel. The IR is accepted by the verifier (opt -passes=verify exits 0), so the backend must not abort. A correct compiler should either (a) treat an internal kernel's byval params with ABI alignment (not over-align them, since a kernel's param buffer is filled by the host/driver which the compiler does not control), or (b) replace the linkage-only assumption with an explicit kernel check in the gate, e.g. `if (!F || !F->hasLocalLinkage() || isKernelFunction(*F) || F->hasAddressTaken(...)) return ABITypeAlign;`. Emitted PTX should declare the param at natural ABI alignment, e.g. `.param .align 4 .b8 ik_param_0[4]`, rather than crashing (debug) or silently emitting `.param .align 16` (release).

## Verification

Independent verify + adversarial refute, both `confirmed` (verify confidence 0.98).

> Confirmed assertion failure on valid, non-UB IR. The cited mechanism is real and reproduces exactly.

Source path (verified in /Users/justinlebar/code/llvm2/llvm/lib/Target/NVPTX, the source the built llc compiled from):
- NVPTXUtilities.cpp:35-53 getFunctionParamOptimizedAlign: gate at line 44 returns ABI align only if `!F || !F->hasLocalLinkage() || hasAddressTaken(...)`. An `internal` function has local linkage, so `!hasLocalLinkage()` is false and (not address-taken) it falls through to line 51 `assert(!isKernelFunction(*F) && "Expect kernels to have non-local linkage")`.
- isKernelFunction (NVVMProperties.h:34-36) returns true iff `F.getCallingConv() == CallingConv::PTX_Kernel`. So an internal ptx_kernel triggers the assert.

Reached via NVPTXSetByValParamAlign.cpp: setByValParamAlignment (line 140-144) iterates byval args -> setByValParamAlign -> getFunctionParamOptimizedAlign. (Also reachable from NVPTXAsmPrinter.cpp:1414/1430 on the IsKernelFunc byval path, but the SetByValParamAlign pass runs first and aborts.)

Adversarial checks done:
1. IR validity: `opt -passes=verify` exits 0 and round-trips the IR cleanly. The verifier accepts internal ptx_kernel with a byval param. Not invalid IR.
2. No UB: the function body is just `ret void`; no loads/stores, no poison/undef/freeze. The crash occurs in the codegen pass before any execution semantics matter, so there is no UB e
