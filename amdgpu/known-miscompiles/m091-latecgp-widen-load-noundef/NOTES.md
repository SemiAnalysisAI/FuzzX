# m091: AMDGPULateCodeGenPrepare copies `!noundef` from sub-DWORD load onto widened i32 load (bits outside the original load are NOT noundef)

*Discovery method: code inspection.*

## The bug

`AMDGPULateCodeGenPrepare.cpp:499-552` widens any uniform sub-DWORD
constant-AS load that is naturally aligned but not DWORD-aligned to an
i32 load at the dword-down address. The new i32 load is created from
scratch with `Builder.CreateAlignedLoad(getInt32Ty(), NewPtr, Align(4))`,
then `copyMetadata(LI)` clones every metadata kind from the original
narrow load, then `setMetadata(MD_range, nullptr)` clears `!range`.

```cpp
// AMDGPULateCodeGenPrepare.cpp, visitLoadInst:
LoadInst *NewLd = IRB.CreateAlignedLoad(IRB.getInt32Ty(), NewPtr, Align(4));
NewLd->copyMetadata(LI);
NewLd->setMetadata(LLVMContext::MD_range, nullptr);
```

`MD_range` is special-cased -- a width-mismatched range MD would be
rejected by the IR Verifier (the m082 bug, in another pass). But
**`MD_noundef` is not cleared**, even though it transplants a "every bit
of this load is well-defined" claim from a load that read N<32 bits to a
load that reads 32 bits. Bits `[N, 32)` of the widened load come from
neighbouring bytes whose noundef-ness is *not* implied by the original
attribute.

This is the same shape as m088 (`AMDGPULowerKernelArguments` widening
sub-DWORD kernargs to i32 and stamping `!noundef`); the present pass
reaches the same broken state via `copyMetadata` rather than an explicit
`setMetadata` call.

The matching pattern exists in `AMDGPUCodeGenPrepare.cpp:1561-1562` as
well (`WidenLoad->copyMetadata(I); ... setMetadata(MD_range, ...)`).
That pass's `WidenLoads` cl::opt defaults to `false`, so it's latent in
stock pipelines but immediately weaponisable with
`-amdgpu-codegenprepare-widen-constant-loads=true`. The
LateCodeGenPrepare one is the dangerous one: its `WidenLoads` cl::opt
defaults to `true`.

`AMDGPULateCodeGenPrepare` runs in
`AMDGPUPassConfig::addPreEmitPass`/`addCodeGenPrepare` (late in the
pipeline), after the IR-level optimiser. Same caveat as m088: stock
Clang -O2 doesn't run InstCombine/GVN after it, so the freeze-drop
weaponisation doesn't fire under `clang -O2 -c`; opt invocations with a
hand-rolled pipeline, LTO post-link re-optimisation, and downstream JITs
that round-trip the lowered IR all hit it.

## Reproducer (showing the bad metadata)

`/tmp/findbug/mdaudit/reduced.ll`:

```llvm
target triple = "amdgcn-amd-amdhsa"

define amdgpu_kernel void @fuzz_kernel(ptr addrspace(4) align 4 %p,
                                       ptr addrspace(1) %out) #0 {
entry:
  %p1 = getelementptr inbounds i8, ptr addrspace(4) %p, i64 1
  %v  = load i8, ptr addrspace(4) %p1, align 1, !noundef !0
  %vz = zext i8 %v to i32
  store i32 %vz, ptr addrspace(1) %out, align 4
  ret void
}

attributes #0 = { nounwind "target-cpu"="gfx950"
                  "uniform-work-group-size"="true" }
!0 = !{}
```

```
$ /opt/rocm-7.1.1/lib/llvm/bin/opt -S \
    -passes=amdgpu-late-codegenprepare \
    -mtriple=amdgcn-amd-amdhsa -mcpu=gfx950 reduced.ll
...
  %0 = getelementptr i8, ptr addrspace(4) %p, i64 0
  %1 = load i32, ptr addrspace(4) %0, align 4, !noundef !0   ; <-- on i32
  %2 = lshr i32 %1, 8
  %3 = trunc i32 %2 to i8
  ...
```

`!noundef` is now attached to an i32 load whose bits `[0, 8) ∪ [16, 32)`
were never seen by the original `!noundef`-annotated i8 load.

## Reproducer (weaponising via dropped freeze)

`/tmp/findbug/mdaudit/reduced_weapon.ll`: two adjacent i8 loads (offsets
+1 and +2) from the same constant pointer. Only the `+1` load carries
`!noundef`. The `+2` byte is consumed via `freeze` + branch.

```
$ /opt/rocm-7.1.1/lib/llvm/bin/opt -S \
    -passes='amdgpu-late-codegenprepare,gvn,instcombine' \
    -mtriple=amdgcn-amd-amdhsa -mcpu=gfx950 reduced_weapon.ll
...
  %0 = load i32, ptr addrspace(4) %p, align 4, !noundef !0
  %1 = lshr i32 %0, 8
  %vz = and i32 %1, 255
  store i32 %vz, ptr addrspace(1) %out, align 4
  %2 = and i32 %0, 16711680                ; byte at offset +2
  %cmp.not = icmp eq i32 %2, 0
  br i1 %cmp.not, label %else, label %then   ; <-- freeze GONE
```

Control (`reduced_weapon_control.ll`, identical kernel but the `+1` load
lacks `!noundef`):

```
  %0 = load i32, ptr addrspace(4) %p, align 4
  %.fr1 = freeze i32 %0                    ; <-- freeze RETAINED
  ...
  br i1 %cmp.not, label %else, label %then
```

The only IR-level difference between the two inputs is `!noundef` on the
`+1` byte. That attribute should constrain only that byte; instead it
makes the optimiser drop a `freeze` on bits `[16, 24)` (the `+2` byte).
Branching on a possibly-poison value is immediate UB.

## Toolchain Results

| Toolchain                                                  | Widened i32 load gets `!noundef`? | Freeze dropped by GVN+InstCombine? |
| ---------------------------------------------------------- | --------------------------------- | ---------------------------------- |
| ROCm 7.1.1 (`/opt/rocm-7.1.1/.../opt`)                     | Yes                               | Yes                                |
| ROCm 7.2.3 (`build/rocm-7.2.3-extract/.../opt`)            | Yes                               | (same pipeline, same outcome)      |

## Fix sketch

Mirror the existing `setMetadata(MD_range, nullptr)` line:

```cpp
NewLd->copyMetadata(LI);
NewLd->setMetadata(LLVMContext::MD_range, nullptr);
NewLd->setMetadata(LLVMContext::MD_noundef, nullptr);   // <-- add this
```

(Pointer-only kinds -- `nonnull`, `dereferenceable*`, `align` -- cannot
be present on a sub-DWORD load because the load's type isn't a pointer,
so they don't need clearing.) Apply the same fix in
`AMDGPUCodeGenPrepare.cpp:1562`.

## Cases ruled out while auditing

* `AMDGPULowerKernelAttributes.cpp:101/117/140/436/438`: m082 covered the
  type-width bugs; m089 covered the floor/ceil bug. The remaining
  range-MD sites width-check the load type before emitting, and the
  `MD_noundef` at line 438 is attached to a freshly-created i32 load
  whose entire dword really is well-defined (`hidden_block_count_x` is
  set unconditionally by the runtime). Sound.
* `AMDGPULowerKernelArguments.cpp:190/198/315/320/325/331`: alias-scope
  / noalias MD covered by m083; the m082/m088 issues here are already
  documented.
* `AMDGPULowerExecSync.cpp:81`,
  `AMDGPULowerModuleLDSPass.cpp:515`,
  `AMDGPUSwLowerLDS.cpp:316`: `MD_absolute_symbol` on a GlobalVariable,
  half-open `[Address, Address+1)` using IntPtrType. Width matches by
  construction and `Address+1` only overflows at `UINT32_MAX`, which is
  excluded.
* `AMDGPULowerModuleLDSPass.cpp:1484/1515`: alias.scope / noalias MD
  attached to instructions accessing the merged LDS struct. The
  scopes/domains are computed inside the pass and the union/intersect
  logic explicitly handles existing MD from prior passes. The MD shape
  is correct.
* `SIISelLowering.cpp:20676`: `MD_noalias_addrspace` set to
  `[PRIVATE_ADDRESS, PRIVATE_ADDRESS+1)` on a `LoadedGlobal` after
  splitting a flat atomic. The MD is a half-open range of address-space
  numbers; type and value sound.
* `AMDGPULibCalls.cpp:1185/1229`: `MD_fpmath` on the new sqrt/rsqrt
  intrinsic call, computed as `max(originalAccuracy, 2.0f)`. Conservative
  (looser ULP cannot tighten downstream analysis incorrectly).
* `AMDGPUSubtarget.cpp:346`: `MD_range` for workitem-id/workgroup-size
  reads, derived from `getReqdWorkGroupSize` and emitted as
  `APInt(32, ...)` only on instructions whose type-width matches (the
  CallBase path uses `addRangeRetAttr`; the instruction path is for
  same-width loads).
* `AMDGPUPromoteAlloca.cpp:1272-1273`: `MD_invariant_load` on the two
  dispatch-packet loads (LoadXY/LoadZU). These read 32-bit fields and
  the dispatch packet is genuinely invariant within a kernel
  invocation.
