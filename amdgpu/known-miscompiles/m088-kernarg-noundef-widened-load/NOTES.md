# m088: AMDGPULowerKernelArguments transplants `noundef` ParamAttr onto widened i32 kernarg load whose high bits are NOT covered

*Discovery method: code inspection.*

## The bug

`AMDGPULowerKernelArguments.cpp:288-320` widens any sub-dword scalar
kernel-argument load to i32 (the `DoShiftOpt` path), then stamps
`!noundef` on the *widened* load whenever the original `Arg` carried a
`noundef` ParamAttr:

```cpp
bool DoShiftOpt = Size < 32 && !ArgTy->isAggregateType();
// ...
if (DoShiftOpt) {
  ArgPtr = Builder.CreateConstInBoundsGEP1_64(
      Builder.getInt8Ty(), KernArgSegment, AlignDownOffset, /*...*/);
  AdjustedArgTy = Builder.getInt32Ty();      // load is now i32
}
// ...
LoadInst *Load = Builder.CreateAlignedLoad(AdjustedArgTy, ArgPtr, AdjustedAlign);
Load->setMetadata(LLVMContext::MD_invariant_load, MDNode::get(Ctx, {}));

if (Arg.hasAttribute(Attribute::NoUndef))
  Load->setMetadata(LLVMContext::MD_noundef, MDNode::get(Ctx, {}));
```

`!noundef` on a load means *every bit of the loaded value is well-defined*
(per `ValueTracking.cpp:7884-7888`, any load with `MD_noundef` is treated
by `isGuaranteedNotToBeUndefOrPoison` as fully noundef). The widened i32
load, however, reads the entire dword containing the sub-dword arg, so
bits `[Size, 32)` come from one of:

1. **A sibling kernarg** in the same dword (e.g. `i8 noundef %a, i1 %b`).
   The sibling has its own (separate) noundef-or-not status; the
   attribute on `%a` says nothing about `%b`'s noundef-ness.
2. **Padding bytes** between explicit args, which the AMDHSA ABI does
   not require the host to initialise.

Compare with the `range` MD block immediately above (m082) and the
pointer-attribute block at lines 329-356: the latter is guarded by
`if (isa<PointerType>(ArgTy))`, so pointer attributes never reach the
widened-load path. `noundef` and `range` (m082) are the two attributes
that *unconditionally* get stamped onto the widened load.

m082's NOTES already flag this as "documented but not reproduced".
This memo weaponises it.

## Why it is wrong

The narrow sub-dword view of the original arg is recovered downstream as
`trunc i32 %load to iN` (lines 359-364). The trunc discards bits
`[Size, 32)`, so callers that look only at the trunc see the correct
noundef-bounded value. BUT any pass that asks
`isGuaranteedNotToBeUndefOrPoison(%load)` — i.e. the un-truncated i32 —
now gets `true`, and that "yes" applies to bits derived from the
*non-noundef* sibling kernarg or padding.

The classic weaponisation: the sibling kernarg `%b` is fed (with a
`freeze` guard) to a branch condition. Source semantics: `freeze` turns
any-possible-poison `%b` into a fixed bit, branch is well-defined.
After the bug-induced `!noundef` lets the optimiser conclude every
trunc-derived value from the load is also noundef, the `freeze` is
dropped (`InstructionCombining.cpp:5249-5265`); the branch now reads a
possibly-poison value directly, which is immediate UB.

## Reproducer

`/tmp/findbug/kernarg_noundef/reduced.ll`:

```llvm
target triple = "amdgcn-amd-amdhsa"

define protected amdgpu_kernel void @fuzz_kernel(i8 noundef %a, i1 %b,
                                                 ptr addrspace(1) %out) #0 {
entry:
  %za = zext i8 %a to i32
  store i32 %za, ptr addrspace(1) %out, align 4
  %fb = freeze i1 %b
  br i1 %fb, label %then, label %else
then:
  %p1 = getelementptr i32, ptr addrspace(1) %out, i64 1
  store i32 1, ptr addrspace(1) %p1, align 4
  ret void
else:
  %p2 = getelementptr i32, ptr addrspace(1) %out, i64 1
  store i32 2, ptr addrspace(1) %p2, align 4
  ret void
}

attributes #0 = { nounwind "target-cpu"="gfx950" }
```

## Demonstration -- freeze guarding branch is dropped

```
$ /opt/rocm-7.1.1/lib/llvm/bin/opt -S \
    -passes='amdgpu-lower-kernel-arguments,gvn,instcombine' \
    -mtriple=amdgcn-amd-amdhsa -mcpu=gfx950 reduced.ll
...
  %0 = load i32, ptr addrspace(4) %fuzz_kernel.kernarg.segment,
                 align 16, !invariant.load !0, !noundef !0
  %1 = and i32 %0, 256
  %.not = icmp eq i32 %1, 0
  ...
  br i1 %.not, label %else, label %then     ; <-- freeze GONE
```

Compare with the same kernel but `%a` lacking `noundef`
(`test3_control.ll` in `/tmp/findbug/kernarg_noundef/`):

```
  %0  = load i32, ptr addrspace(4) ..., align 16, !invariant.load !0
  %.fr = freeze i32 %0                        ; <-- freeze RETAINED
  %1   = and i32 %.fr, 256
  %.not = icmp eq i32 %1, 0
  br i1 %.not, label %else, label %then
```

The single difference between the two inputs is the `noundef` attribute
on `%a`. That attribute should constrain only the low byte (`%a`), yet
it causes the optimiser to drop a `freeze` on bit 8 (`%b`).

## Toolchain Results

| Toolchain                                                  | Bug present in lowered IR | Freeze dropped by GVN+InstCombine? |
| ---------------------------------------------------------- | ------------------------- | ---------------------------------- |
| LLVM HEAD / FuzzX patches (`build/llvm-fuzzer`)            | Yes (`!noundef` on i32 load) | Yes (verified via opt-equivalent run) |
| ROCm 7.2.3 (`build/rocm-7.2.3-extract/.../opt`)            | Yes                          | Yes                                |
| ROCm 7.1.1 (`/opt/rocm-7.1.1/lib/llvm/bin/opt`)            | Yes                          | Yes                                |

## Why the stock Clang -O2 pipeline does not weaponise this today

`AMDGPULowerKernelArguments` runs from `AMDGPUPassConfig::addCodeGenPrepare`
(`AMDGPUTargetMachine.cpp:1530-1538`), AFTER the IR-level O2 pipeline.
Stock Clang -O2 schedules no InstCombine/GVN pass *after*
lower-kernel-arguments, so the freeze drop never happens in `clang -O2 -c`.
The bug bites when:

* `opt` is invoked with a hand-rolled pipeline (research/testing); or
* an LTO post-link scheme re-runs IR opts after CGP; or
* a downstream tool (e.g. an in-house JIT or a re-optimiser) round-trips
  the lowered IR through GVN+InstCombine.

The IR is invalid IR-language-wise in all of those settings; the lack of
a downstream pass in *one specific* pipeline is luck, not correctness.
Compare with m082 which has the same "latent until re-optimised" flavour
and is documented as a bug.

The Verifier does NOT check `!noundef` MD soundness (it is a nullary MD;
its semantics are "trust the producer"), so unlike m082's `!range` width
mismatch, this slips past `--verify-each` as well.

## How a fix should look

Mirror the `range`-MD fix (option 1 in m082): drop the `!noundef` stamp
on the `DoShiftOpt` path. The narrow value the user actually consumes is
the trunc, and downstream analysis can re-derive noundef-ness of the
trunc from the `Arg`'s `noundef` attribute on the surrounding code or
from `assume` intrinsics inserted by attributor.

Equivalently, hoist the `setMetadata(MD_noundef, ...)` after the
`DoShiftOpt` block, attaching it to the `trunc` user instead of the
load. That keeps the optimisation power for the narrow value without
making a false claim about the high bits.

## Cases ruled out while auditing

* `range` MD (lines 322-327): covered by m082; same root cause but
  produces invalid IR (Verifier-rejected) in addition to being
  semantically wrong.
* Pointer attributes (`nonnull` / `dereferenceable` /
  `dereferenceable_or_null` / `align`, lines 329-356): guarded by
  `isa<PointerType>(ArgTy)`. Pointers are always >= 32 bits so
  `DoShiftOpt` is false; never reach the widened-load path.
* `IsV3` widening (lines 307-311): the v3->v4 hack only applies when
  `Size >= 32`, so it does not interact with the sub-dword `DoShiftOpt`
  metadata path.
* Byref args (line 253): take the address-cast path; no load emitted; no
  MD issue.
* Inreg / preload args (line 248): skipped entirely; preload pass
  handles them separately.
* `AMDGPULowerKernelAttributes.cpp:438` (the other site that stamps
  `MD_noundef` on a load): attaches `MD_noundef` to the result of
  `__ockl_get_local_size`/etc., where the language-level value really
  is fully defined; no widening, no width mismatch.
* `AMDGPUPreloadKernelArguments.cpp`: does not emit loads; only marks
  args `inreg` and clones the signature. Offset arithmetic mirrors
  `AMDGPULowerKernelArguments.cpp`.
* SelectionDAG: does not consume `!noundef` MD (verified via grep over
  `lib/CodeGen/SelectionDAG/`). The miscompile is purely mid-level.

## Why not a runtime HIP O0/O2 mismatch?

The harness in `known-miscompiles/run_ll_reproducer.sh` invokes
`clang -O0` and `clang -O2` end-to-end. As noted above, stock Clang -O2
does not schedule an IR opt pass after `amdgpu-lower-kernel-arguments`,
so the freeze-drop transform that exposes the bug does not fire in
either O0 or O2. A runtime diff would require either:

* an LTO post-link scheme; or
* a follow-up `opt -passes='gvn,instcombine'` invocation between Clang
  and `llc`.

Neither is what `run_ll_reproducer.sh` does, so the weaponisation here
is delivered via the opt + asm diff shown above (mirroring m082 and
m083, which similarly rely on `opt -S` rather than HIP execution).
