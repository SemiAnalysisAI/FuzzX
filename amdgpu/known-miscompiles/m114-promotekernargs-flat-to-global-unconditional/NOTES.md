# m114: `AMDGPUPromoteKernelArguments` unconditionally addrspacecasts flat kernarg pointers to global, silently miscompiling LDS/private-aperture inputs

*Discovery method: code inspection.*  Sibling shape to m088/m091 -- a
mid-level pass making an unwarranted assumption about kernarg
semantics.

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/AMDGPUPromoteKernelArguments.cpp:105-128`
(`promoteFlatArgument`):

```cpp
// For every kernarg with type ptr in FLAT_ADDRESS:
auto *Cast0 = new AddrSpaceCastInst(P, GlobalPtrTy, "", BB->getFirstInsertionPt());
auto *Cast1 = new AddrSpaceCastInst(Cast0, FlatPtrTy, "", BB->getFirstInsertionPt());
P->replaceUsesWithIf(Cast1, [&](Use &U) { ... });
```

The pass wraps every `FLAT_ADDRESS` kernel-arg pointer in
`addrspacecast(addrspacecast(p to ptr addrspace(1)) to ptr)` so
`InferAddressSpaces` (run next in the pipeline) will rewrite the
downstream memops to `global_*`.

There is NO check that the flat pointer is actually in the global
aperture.  A flat kernarg can legitimately carry a pointer whose
aperture is **LOCAL** (AS 3) or **PRIVATE** (AS 5) -- e.g. the host
sets `addrspacecast(@LDS to ptr)` and passes that into the kernel
as a flat `ptr`.

Per LangRef:

> If the source value's address space and the destination address space
> are not in the same address space hierarchy, the resulting pointer
> value is undefined behavior.

AMDGPU's flat aperture is a single virtual range; casting flat -> global
strips the aperture base, so the resulting "global" address is garbage.
The subsequent `global_store` then hits arbitrary global memory instead
of the LDS / scratch slot.

## Reproducer

`reduced.ll`:

```llvm
define amdgpu_kernel void @k(ptr addrspace(1) readonly %pp, ptr %dst) {
  store i32 7, ptr %dst, align 4
  ret void
}
```

`opt -S -passes='amdgpu-promote-kernel-arguments,infer-address-spaces'
reduced.ll`:

```llvm
%0 = addrspacecast ptr %dst to ptr addrspace(1)
store i32 7, ptr addrspace(1) %0, align 4
```

Then `llc -mtriple=amdgcn-amd-amdhsa -mcpu=gfx950`:

```asm
global_store_dword v0, v1, s[0:1]    ; original would have been flat_store
```

The original `flat_store_dword` would have correctly inspected the
aperture and dispatched to LDS / scratch / global.  The promoted
`global_store_dword` bypasses the flat aperture entirely.

The companion case `repro_local.ll` (kernarg slot holds an LDS-aperture
flat ptr loaded from another global) exhibits the same promotion --
also miscompiled.

## Why this matters for default pipeline

`AMDGPUPromoteKernelArguments` runs at `-O2` for the SDAG path on every
AMDGPU target (registered in `AMDGPUTargetMachine.cpp`).  Any client
that passes flat pointers into a kernel with a non-global aperture is
affected.

## Suggested fix

Skip the cast unless we can prove the loaded/argument pointer is in the
global aperture.  Options:

1. Restrict to kernargs that have an explicit aperture-asserting
   attribute (e.g. `!amdgpu.global` metadata or a new `addrspace(1)`
   constraint).
2. Use `InferAddressSpaces`'s own analysis: only promote when the flat
   pointer's aperture can be proven `GLOBAL` via the existing flat-AS
   resolver.
3. Conservative: gate on a target opt-in flag for clients that
   guarantee all flat kernargs are global.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Reproduces. |
| ROCm 7.1.1 (`/opt/rocm-7.1.1/lib/llvm/bin/opt`) | Same pass, same bug. |

## Why the fuzzer hasn't caught it

* The current IR fuzzer does not emit kernargs that are flat-typed
  pointers pointing into LDS / private.  All kernarg pointers are
  `ptr addrspace(1)` (global) directly.
* The interpreter oracle assumes flat kernargs are global.
* Per `MEMORY.md` (Prefer-random-over-idioms), the right hook is to
  emit kernargs typed as `ptr` (flat) with a runtime `addrspacecast`
  from `@LDS` symbols so the pass sees aperture diversity.
