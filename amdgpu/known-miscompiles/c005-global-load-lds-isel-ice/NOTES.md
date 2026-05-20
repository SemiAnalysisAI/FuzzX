# c005: `llvm.amdgcn.global.load.lds` ICEs in AMDGPU instruction selection on gfx950

The "global-load-to-LDS" intrinsic is documented as a GFX9.4 / GFX10+
data-movement primitive that copies bytes from a global address directly
into LDS without going through VGPRs.  On `gfx950` the intrinsic itself
compiles cleanly for valid byte sizes (1, 2, 4) -- but when the `size`
immediate is `0`, the SDAG matcher aborts with `Cannot select` instead of
emitting a clean diagnostic.

`size=0` is not meaningful so this is unreachable in well-formed source,
but a defended backend should produce a verifier error or a clean
"unsupported intrinsic argument" message rather than `report_fatal_error`.

```bash
known-miscompiles/run_ll_compiler_reproducer.sh \
  known-miscompiles/c005-global-load-lds-isel-ice/reduced.ll
```

Observed output:

```text
O0=fail
O0-exit=1
O0-message=fatal error: error in backend: Cannot select: intrinsic %llvm.amdgcn.global.load.lds
O2=fail
O2-exit=1
O2-message=fatal error: error in backend: Cannot select: intrinsic %llvm.amdgcn.global.load.lds
compiler_failure=true
```

This is in the same family as c003 (`permlane16`) and c004 (`mov.dpp8`):
intrinsics declared target-unconditionally in `IntrinsicsAMDGPU.td` that
the SDAG matcher cannot lower for CDNA targets.  Two more siblings
discovered during the same sweep but not yet given separate entries:

| Intrinsic | gfx950 behaviour |
| --- | --- |
| `llvm.amdgcn.ds.ordered.add` | ICE: `Cannot select: AMDGPUISD::DS_ORDERED_COUNT` (GDS-related, RDNA/GFX10+) |
| `llvm.amdgcn.image.bvh.intersect.ray` | clean diagnostic ("intrinsic not supported on subtarget") -- this is the desired shape for the others |

The contrast with `image.bvh.intersect.ray` shows the AMDGPU backend already
knows how to give a clean "intrinsic not supported on subtarget" diagnostic
when the lowering code path properly checks the subtarget feature; the c003
/ c004 / c005 family needs the same predicate plumbing.

## Fuzzer Suppression

Not yet wired up.  Add a `c005`-style suppressor in
`fuzzer/llvm_amdgpu_diff_fuzzer.cpp` to drop
`Intrinsic::amdgcn_global_load_lds` (and `amdgcn_ds_ordered_add`) from any
IR generator that targets CDNA, mirroring c001/c002/c003/c004.
