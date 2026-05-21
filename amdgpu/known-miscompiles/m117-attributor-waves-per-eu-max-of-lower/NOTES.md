# m117: `AAAMDWavesPerEU` propagates `max(lower)` instead of `min(lower)` -- callee inherits tightest kernel's register budget

*Discovery method: code inspection.*

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/AMDGPUAttributor.cpp:1165-1170`:

```cpp
ConstantRange Assumed = getAssumed();
unsigned Min = std::max(Assumed.getLower().getZExtValue(),
                        CallerAA->getAssumed().getLower().getZExtValue());
unsigned Max = std::max(Assumed.getUpper().getZExtValue(),
                        CallerAA->getAssumed().getUpper().getZExtValue());
ConstantRange Range(APInt(32, Min), APInt(32, Max));
IntegerRangeState RangeState(Range);
getState() = RangeState;          // overwrites instead of clamping (non-monotonic)
```

For a callee reached from multiple kernels the propagated range should
be the union `[min(lower_i), max(upper_i)]`.  The code uses `max` on
both endpoints.  A callee shared by `[1,1]` and `[8,8]` becomes `[8,8]`
-- the high-occupancy kernel's tight register budget is imposed on the
callee even when invoked from the relaxed kernel.

Two adjacent defects on the same lines:

1. `getState() = RangeState` overwrites assumed, defeating monotonic
   clamp -- state can grow then shrink across iterations, leading to
   fixpoint instability.
2. The trailing equality check at line 1173 compares
   `IntegerRangeState` to `ConstantRange` via implicit conversion --
   almost certainly not the intended equality.

The sibling helper `AAAMDSizeRangeAttribute::updateImplImpl` (line
838) uses `clampStateAndIndicateChange` and is correct; this AA
reinvents the wheel incorrectly.

## Reproducer

`reduced.ll`:

```llvm
define internal void @callee(...) { store ... ret void }
define amdgpu_kernel void @k_tight(...)   #0 { call ... }   ; "1,8" -> wrong "8,8"
define amdgpu_kernel void @k_relaxed(...) #1 { call ... }
attributes #0 = { "amdgpu-waves-per-eu"="8,8" }
attributes #1 = { "amdgpu-waves-per-eu"="1,1" }
```

`opt -mtriple=amdgcn-amd-amdhsa -mcpu=gfx950 -passes=amdgpu-attributor -S reduced.ll`:

* Expected: `@callee` carries `"amdgpu-waves-per-eu"="1,8"`.
* Observed: `@callee` placed in attribute group `#0` with
  `"amdgpu-waves-per-eu"="8,8"` -- same as the tight kernel.

## Impact

* Spills and reduced occupancy in shared device functions when one
  kernel uses a high-occupancy `waves-per-eu` and another uses a low
  one.
* Non-monotonic state transitions can fail to reach fixpoint.

Acknowledged but undirected upstream in
`llvm/test/CodeGen/AMDGPU/min-waves-per-eu-not-respected.ll`.

## Suggested fix

Replace the body with the same `clampStateAndIndicateChange` pattern
used by `AAAMDSizeRangeAttribute::updateImplImpl`.  Or manually:

```cpp
unsigned Min = std::min(Assumed.getLower().getZExtValue(),
                        CallerAA->getAssumed().getLower().getZExtValue());
unsigned Max = std::max(Assumed.getUpper().getZExtValue(),
                        CallerAA->getAssumed().getUpper().getZExtValue());
```

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD (`build/llvm-fuzzer/bin/opt`) | Reproduces. |
| ROCm 7.1.1 | Same buggy combine. |
