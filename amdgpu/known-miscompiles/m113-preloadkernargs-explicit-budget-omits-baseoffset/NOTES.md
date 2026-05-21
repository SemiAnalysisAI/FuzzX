# m113: `AMDGPUPreloadKernelArguments` explicit-arg budget check omits `BaseOffset`

*Discovery method: code inspection (audit a355c2486118afa57).*

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/AMDGPUPreloadKernelArguments.cpp:181-183, 295-329`:

```cpp
// Line 181-183 -- budget predicate, compares against raw NumFreeUserSGPRs*4:
bool canPreloadKernArgAtOffset(uint64_t ExplicitArgOffset) {
  return ExplicitArgOffset <= NumFreeUserSGPRs * 4;
}

// Line 295-329 -- markKernelArgsAsInreg, explicit-arg loop:
uint64_t ExplicitArgOffset = 0;
const DataLayout &DL = F.getDataLayout();
const uint64_t BaseOffset = ST.getExplicitKernelArgOffset();   // <-- read
...
for (Argument &Arg : F.args()) {
  ...
  ExplicitArgOffset = alignTo(ExplicitArgOffset, ABITypeAlign) + AllocSize;
  if (!PreloadInfo.canPreloadKernArgAtOffset(ExplicitArgOffset))  // <-- BUG
    break;                                                        //     BaseOffset
                                                                  //     NOT added
  Arg.addAttr(Attribute::InReg);
  ...
}
```

The hidden-arg path in the same file does pass `BaseOffset` to the
predicate via `ImplicitArgsBaseOffset`:

```cpp
// Line 240-242 -- hidden-arg path, correct:
if (!canPreloadKernArgAtOffset(LoadOffset + LoadSize +
                               ImplicitArgsBaseOffset))
  return true;

// Line 333-336 -- ImplicitArgsBaseOffset is built with BaseOffset:
uint64_t ImplicitArgsBaseOffset =
    alignTo(ExplicitArgOffset, ST.getAlignmentForImplicitArgPtr()) +
    BaseOffset;
```

So the read of `BaseOffset` at line 297 is dead for the explicit-arg
loop -- the explicit-arg budget check at line 322 measures the
running `ExplicitArgOffset` from zero, even though on non-AMDHSA /
non-AMDPAL / non-Mesa3D triples the explicit kernarg segment really
starts at byte `BaseOffset = 36`:

```cpp
// AMDGPUSubtarget.h:254-265:
unsigned getExplicitKernelArgOffset() const {
  switch (TargetTriple.getOS()) {
  case Triple::AMDHSA:
  case Triple::AMDPAL:
  case Triple::Mesa3D:
    return 0;
  case Triple::UnknownOS:
  default:
    return 36;
  }
}
```

The pass therefore over-marks explicit args as `inreg` on unknown-OS
(and any other future non-HSA OS): it claims args fit within
`NumFreeUserSGPRs * 4` SGPRs measured from offset 0, but the
runtime-supplied SGPR preload sequence starts at the beginning of the
input buffer, 36 bytes before the explicit-arg payload.

`SIISelLowering.cpp:3061-3138` (`allocatePreloadKernArgSGPRs`) does
the math correctly --

```cpp
unsigned LastExplicitArgOffset = Subtarget->getExplicitKernelArgOffset();
...
unsigned Padding = ArgOffset - LastExplicitArgOffset;
unsigned PaddingSGPRs = alignTo(Padding, 4) / 4;
if (PaddingSGPRs + NumAllocSGPRs > SGPRInfo.getNumFreeUserSGPRs()) {
  InPreloadSequence = false;
  break;
}
```

so SI-lowering bails partway through the preload sequence on
unknown-OS.  But `AMDGPULowerKernelArguments.cpp:247-249`:

```cpp
// Skip inreg arguments which should be preloaded.
if (Arg.use_empty() || Arg.hasInRegAttr())
  continue;
```

unconditionally trusts the `inreg` attribute and skips lowering those
args.  Result: args that the IR pass marked `inreg`, but that SI
lowering refused to preload (because they fell off the back of the
real SGPR budget), are simply never lowered -- the SDAG-side `MemLoc`
machinery still allocates them in the runtime SGPR preload range
(starting at offset 0 of the input buffer), so the kernel reads
`NumFreeUserSGPRs * 4` bytes of runtime-header / pre-segment garbage
instead of its actual arguments.

## Reproducer

`reduced.ll`:

```llvm
target triple = "amdgcn--"

define amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %out,
                                       i32 %a0, i32 %a1, ..., i32 %aE) #0 {
entry:
  ; 15 stores of %a0..%aE into %out[0..14]
  ret void
}

attributes #0 = { nounwind "amdgpu-no-implicitarg-ptr" "target-cpu"="gfx950" }
```

`opt -mcpu=gfx950 -amdgpu-kernarg-preload-count=16
-passes='amdgpu-preload-kernel-arguments,function(amdgpu-lower-kernel-arguments)'
-S reduced.ll`:

```llvm
define amdgpu_kernel void @fuzz_kernel(
    ptr addrspace(1) inreg %out,            ; <-- marked inreg
    i32 inreg %a0, ..., i32 inreg %a5,      ; <-- marked inreg
    i32 %a6, ..., i32 %aE) #0 {             ; <-- NOT marked
  ; %a6..%aE lowered to kernarg.segment loads (offsets 68..100,
  ; correctly including the 36-byte BaseOffset).
  ; %out, %a0..%a5 have no lowering at all -- the kernel assumes
  ; they live in the preloaded SGPRs.
  ...
}
```

`llc -mcpu=gfx950 -amdgpu-kernarg-preload-count=16 reduced.ll -o -`:

```asm
fuzz_kernel:
    s_load_dwordx8 s[8:15],  s[4:5], 0x0   ; <-- loads bytes [0..32) of
                                           ;     the input buffer into the
                                           ;     SGPRs the kernel uses as
                                           ;     %out + %a0..%a5; on unknown-OS
                                           ;     these bytes are *runtime header
                                           ;     padding* before the kernarg segment.
    s_load_dwordx8 s[16:23], s[4:5], 0x44  ; 0x44 = 36 + 32 = 68 -- correct
                                           ; offset for the non-inreg %a6..%aD.
    s_load_dword   s0,       s[4:5], 0x64  ; 0x64 = 100 -- correct for %aE.
```

Compare with `-mtriple=amdgcn-amd-amdhsa` (same source, same flags):

```asm
    s_load_dwordx8 s[8:15],  s[4:5], 0x0   ; correct -- AMDHSA segment starts at 0.
    s_load_dwordx8 s[16:23], s[4:5], 0x20  ; 0x20 = 32 -- correct for AMDHSA.
```

Same encoded `s_load_dwordx8 ... 0x0` instruction, but on unknown-OS
the runtime puts the kernarg segment at offset 36, so the first load
reads garbage into the SGPRs the kernel actually uses.

## Why this matters in the default pipeline

`AMDGPUPreloadKernelArguments` runs at `-O > 0` for any GFX target
that has `hasKernargPreload()` (gfx940 / gfx942 / gfx950 + newer
gfx12 chips), and `KernargPreload` is on by default
(`AMDGPUPreloadKernelArguments.cpp:40-43`).

The standard FuzzX harness (`run_ll_reproducer.sh`) and the typical
ROCm front-end emit `amdgcn-amd-amdhsa` triples, so this divergence
is invisible on the default path.  But:

* Out-of-tree front-ends (graphics shaders compiled to `amdgcn-mesa-mesa3d`
  hit `BaseOffset = 0`, but `amdgcn-` / `amdgcn-unknown-unknown` (used
  for some bring-up flows, AMDGPU.bc compile tests, and the in-tree
  `llc` test suite) hit `BaseOffset = 36`.
* The in-tree LLVM `llc` tests under
  `llvm/test/CodeGen/AMDGPU/preload-kernel-arguments-*.ll` mostly use
  `-mtriple=amdgcn-amd-amdhsa`, so the regression is not caught by
  the upstream lit suite either.
* Any future OS-string that defaults to `BaseOffset = 36` (e.g. an
  out-of-tree port) will trip this.

The bug is latent on the default ROCm path but easy to weaponize by
flipping the triple to `amdgcn--`.

## Suggested fix

Add `BaseOffset` to the explicit-arg budget check, mirroring the
hidden-arg path:

```cpp
// Line 322:
if (!PreloadInfo.canPreloadKernArgAtOffset(ExplicitArgOffset + BaseOffset))
  break;
```

(Equivalently: initialize `uint64_t ExplicitArgOffset = BaseOffset;`
before the loop and leave the rest unchanged.  Either approach moves
the explicit-arg loop's budget into the same coordinate system that
SI-lowering's `LastExplicitArgOffset = ST.getExplicitKernelArgOffset()`
already uses.)

With the fix, on unknown-OS / gfx950 the pass marks 0 explicit args
as `inreg` (the first arg's offset = 36 + 8 = 44 > the natural budget
remaining after the 9-SGPR BaseOffset overhead has been accounted
for, depending on `NumFreeUserSGPRs`), which matches what SI-lowering
will actually be able to preload.  No args end up `inreg` without
also being preloaded, so `AMDGPULowerKernelArguments` never silently
drops a lowering.

A secondary hardening worth considering: make
`AMDGPULowerKernelArguments` re-lower any `inreg` arg whose
corresponding `PreloadKernArgs` entry is empty in
`SIMachineFunctionInfo` (defence-in-depth against future drift
between the IR pass and SI-lowering).

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Reproduces (asm shows `s_load_dwordx8 s[8:15], s[4:5], 0x0` on `amdgcn--`; corresponding AMDHSA build has same encoded load but kernarg layout differs). |
| ROCm 7.1.1 (`/opt/rocm-7.1.1/lib/llvm/bin/opt` + `llc`) | Same fold present. |
| ROCm 7.2.3 (`/opt/rocm-7.2.3/lib/llvm/bin/opt` + `llc`) | Same fold present. |

## Why the fuzzer hasn't caught it

* The FuzzX harness compiles+runs through HIP, which forces
  `amdgcn-amd-amdhsa` (`BaseOffset = 0`).  The bug only triggers on
  non-HSA triples, which the harness never produces.
* `run_ll_reproducer.sh` hard-codes `-target amdgcn-amd-amdhsa` in its
  `clang` invocation.
* Per `MEMORY.md` (Prefer-random-over-idioms), the right hook is to
  let the IR fuzzer also emit `target triple = "amdgcn--"` (and
  `amdgcn-mesa-unknown`, etc.) and run a triple-divergence
  differential against AMDHSA.  None of the existing emitters vary
  the triple.
* Even on AMDHSA, a *separate* SDAG-vs-GISel differential on the
  same kernel + `-amdgpu-kernarg-preload-count=N` would catch the
  asymmetric handling between the IR pass and SI-lowering, but the
  fuzzer currently runs only one ISel path per kernel.
