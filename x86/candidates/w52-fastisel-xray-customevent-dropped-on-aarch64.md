# w52: FastISel silently drops llvm.xray.customevent / llvm.xray.typedevent on AArch64-64

## Pattern
"FastISel fall-back path that drops a side-effect intrinsic" — matches the
hunt pattern exactly. (Found while auditing the three listed SelectionDAG
files; the affected target is AArch64, not x86. Filing anyway because the
bug pattern is genuine and the offending code lives in the target-independent
file that the hunt assigned.)

## Location
`llvm/lib/CodeGen/SelectionDAG/FastISel.cpp`, lines 900-903 and 919-922:

```cpp
bool FastISel::selectXRayCustomEvent(const CallInst *I) {
  const auto &Triple = TM.getTargetTriple();
  if (Triple.isAArch64(64) && Triple.getArch() != Triple::x86_64)
    return true; // don't do anything to this instruction.
  ...
}

bool FastISel::selectXRayTypedEvent(const CallInst *I) {
  const auto &Triple = TM.getTargetTriple();
  if (Triple.isAArch64(64) && Triple.getArch() != Triple::x86_64)
    return true; // don't do anything to this instruction.
  ...
}
```

## Why it's wrong
`Triple.isAArch64(64)` is true only when the triple is AArch64-64. In that
case `getArch() != Triple::x86_64` is necessarily true (since the arch is
`aarch64`, not `x86_64`). So the combined predicate degenerates to:

> "If the triple is 64-bit AArch64, return true without emitting anything."

But `llvm.xray.customevent` / `llvm.xray.typedevent` are side-effect
intrinsics with a well-defined lowering on AArch64
(`PATCHABLE_EVENT_CALL` / `PATCHABLE_TYPED_EVENT_CALL` — see
`llvm/lib/Target/AArch64/AArch64AsmPrinter.cpp` lines 594, 3598-3601 and
`llvm/lib/Target/AArch64/AArch64ISelLowering.cpp` lines 3464-3465). On
AArch64-64 with FastISel engaged (`-O0`), these intrinsics are silently
swallowed: no machine instruction is emitted, and the resulting binary
will not record the xray event the program asked for.

`selectIntrinsicCall` dispatches `Intrinsic::xray_customevent` /
`xray_typedevent` to these two functions (lines 1434-1437), so the
side-effect is genuinely dropped — not just deferred to SDAG: the function
returns `true`, which tells `selectInstruction` "I handled it" and no
fallback occurs.

## Likely intent
A correct guard would be the inverse: "skip on targets that lack a
patchable-event lowering". Looking at the only two backends that actually
implement these opcodes (x86_64 and AArch64), the predicate probably wanted
to read something like:

```cpp
if (!Triple.isAArch64(64) && Triple.getArch() != Triple::x86_64)
  return true; // unsupported target, drop quietly
```

i.e. an early-out for *unsupported* targets, not for one of the two
supported ones.

## Impact
* `-O0` build (which uses FastISel) on AArch64-64.
* Any function annotated with `xray-instruction-threshold` / `xray-attr` etc.
  that contains `llvm.xray.customevent(...)`/`xray.typedevent(...)`.
* The expected `PATCHABLE_EVENT_CALL` / `PATCHABLE_TYPED_EVENT_CALL` is
  never emitted. The runtime hook is silently never reached. There is no
  diagnostic.

## NOT impacted
* x86_64: the predicate's first conjunct (`isAArch64(64)`) is false, so the
  function proceeds to emit the patchable-event MI. No bug on x86.
* SDAG path: `SelectionDAGBuilder` lowers these intrinsics directly to
  `PATCHABLE_EVENT_CALL` / `PATCHABLE_TYPED_EVENT_CALL` machine SDNodes; no
  triple guard. So the bug is FastISel-specific.
* AArch64 GISel: independent path.

## Suggested fix
Either invert the condition (drop only on truly-unsupported targets) or
remove the guard entirely — the existing `BuildMI` is target-independent
(it emits a generic `TargetOpcode::PATCHABLE_EVENT_CALL`) and works on any
target that lowers that pseudo. The `getRegForValue` for the two scalar
args is also target-independent and harmless on unsupported targets (it
would either succeed or trigger the normal FastISel bail-out).

## Repro sketch (would need AArch64 backend in this build to confirm)
```ll
target triple = "aarch64-unknown-linux-gnu"

declare void @llvm.xray.customevent(ptr, i32)

define void @f(ptr %p, i32 %sz) #0 {
  call void @llvm.xray.customevent(ptr %p, i32 %sz)
  ret void
}

attributes #0 = { "function-instrument"="xray-always" "xray-skip-entry" "xray-skip-exit" }
```
`llc -O0 -fast-isel=true` (and `-mtriple=aarch64-...`) would emit `ret`
with no `PATCHABLE_EVENT_CALL`. Cannot reproduce from this checkout because
the build only includes the x86/AMDGPU backends; verification was by code
reading + cross-checking the AArch64 PATCHABLE_EVENT_CALL lowering path.

## Cross-file evidence the lowering does exist on AArch64
* `llvm/lib/Target/AArch64/AArch64AsmPrinter.cpp:594` — `LowerPATCHABLE_EVENT_CALL(MI, /*Typed=*/false)`
* `llvm/lib/Target/AArch64/AArch64AsmPrinter.cpp:3598-3601` — dispatch for
  `TargetOpcode::PATCHABLE_EVENT_CALL` and `PATCHABLE_TYPED_EVENT_CALL`
* `llvm/lib/Target/AArch64/AArch64ISelLowering.cpp:3464-3465` — emission
  for both opcodes
* `llvm/lib/Target/AArch64/AArch64InstrInfo.cpp:227-231` — size estimation
  for both opcodes

The check in FastISel was almost certainly intended to *skip non-x86/non-AArch64*
targets (the only two that have working lowering), not to actively skip one of
the two supported targets.
