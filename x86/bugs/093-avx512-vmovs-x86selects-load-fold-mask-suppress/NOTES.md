# w37: VMOVSSZrmk/VMOVSDZrmk/VMOVSHZrmk fold unconditional scalar load
under a mask, suppressing the load when mask=0

File / lines:
- `llvm/lib/Target/X86/X86InstrAVX512.td:4321-4327` (VMOVSSZrmk / VMOVSSZrmkz)
- `llvm/lib/Target/X86/X86InstrAVX512.td:4339-4345` (VMOVSDZrmk / VMOVSDZrmkz)
- corresponding f16 forms (search VMOVSHZrmk* near same area if added)

The Pat<> rewrites:
```
def : Pat<(f32 (X86selects VK1WM:$mask, (loadf32 addr:$src), (f32 FR32X:$src0))),
          (COPY_TO_REGCLASS
           (v4f32 (VMOVSSZrmk (v4f32 (COPY_TO_REGCLASS FR32X:$src0, VR128X)),
                                                       VK1WM:$mask, addr:$src)),
           FR32X)>;
def : Pat<(f32 (X86selects VK1WM:$mask, (loadf32 addr:$src), fp32imm0)),
          (COPY_TO_REGCLASS (v4f32 (VMOVSSZrmkz VK1WM:$mask, addr:$src)), FR32X)>;
```
and the corresponding f64 / f16 forms.

## What's wrong

The IR pattern matched is `X86selects(mask, plain_load addr, src0)` where `loadf32`
/`loadf64`/`loadf16` are vanilla, non-atomic, non-volatile loads.  In LLVM IR /
SelectionDAG semantics both arms of a `select` (and therefore both arms of the
lowered X86selects) are evaluated unconditionally: if the address `addr` is
unmapped or otherwise faulting, evaluating that load is UB-and-fault, regardless
of the mask value.

The chosen instruction `VMOVSSZrmk` / `VMOVSDZrmk` is a MASKED load.  AVX-512
masked loads suppress the load (and any associated fault) when the mask bit is
zero.  Thus folding an unconditional plain load into the masked form means: when
`mask == 0`, the original IR would have faulted on the load, while the rewritten
code returns `src0` (or zero) silently.  This is unsound load-fold for the case
the original load is not provably safe to speculate.

X86selects is produced from generic `ISD::SELECT` of scalar FP values in
`X86ISelLowering.cpp:25795`, which can certainly be a select whose true-arm is
an arbitrary load (see e.g. `select (fcmp lt x, y), (load p), z` patterns).  The
isel pattern relies only on the plain `loadf32` PatFrag (no
`dereferenceable_load` / `simple_load` guard) so there is no isel-level filter
preventing the fold for non-speculatable loads.

Equivalent issue exists for the other-arm forms had they been added, and for
masked store rewrites elsewhere if any unconditional store is folded into a
masked store.

## IR repro idea

```llvm
; Triple: x86_64-unknown-linux-gnu, +avx512f
define float @f(i1 %c, ptr %p, float %s0) {
  %ld = load float, ptr %p          ; may fault, e.g. trap-on-NULL
  %r  = select i1 %c, float %ld, float %s0
  ret float %r
}
```
Run `llc -O2 -mattr=+avx512f`.  If the compiler emits `vmovss (...) {%k1}, %xmm0`
folding the load under the mask, then a NULL `%p` with `%c=false` returns
`%s0` rather than segfaulting (latent — visible only when the underlying memory
is genuinely unmapped at runtime).  Confirm by inspecting asm and looking for
`vmovss ..., %xmm0 {%k...}` (the `m` form) vs a separate `vmovss (mem)` load.

## Severity

Latent — only manifests when a select-of-load is fed a fault-prone pointer in
the false arm.  Often masked by earlier passes hoisting the load, but the
Pat<> rule itself has no `simple_load` / `MachineMemOperand` dereferenceability
check, so the rewrite is unsound by construction.

## Fix sketch

Use a PatFrag that checks `cast<LoadSDNode>(N)->isSimple() &&
TLI.isSafeToSpeculate(N)` (or limit the rule to `nontemporal`-friendly /
intrinsic-sourced masked loads).  An easier hammer is to restrict to cases where
the load has a non-temporal hint or one-use + dereferenceable predicate, or to
drop the load-fold form entirely (the register form is always safe).
