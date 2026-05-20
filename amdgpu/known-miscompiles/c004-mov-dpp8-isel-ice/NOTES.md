# c004: `llvm.amdgcn.mov.dpp8` ICEs in AMDGPU instruction selection on CDNA targets

`v_mov_b32_dpp8` is the GFX10+ DPP8 form of the data-parallel-primitives
move instruction.  Like the m16 permlane variants in c003, the intrinsic
is declared target-unconditional but the instruction itself is only
available on RDNA targets.  When the intrinsic appears in IR built for
CDNA, instruction selection aborts with `Cannot select`.

```bash
known-miscompiles/run_ll_compiler_reproducer.sh \
  known-miscompiles/c004-mov-dpp8-isel-ice/reduced.ll
```

Observed output:

```text
O0=fail
O0-exit=1
O0-message=fatal error: error in backend: Cannot select: intrinsic %llvm.amdgcn.mov.dpp8
O2=fail
O2-exit=1
O2-message=fatal error: error in backend: Cannot select: intrinsic %llvm.amdgcn.mov.dpp8
compiler_failure=true
```

## Target Sweep

Same shape as c003: ICEs on every gfx9xx (CDNA) target, compiles cleanly on
RDNA (`gfx1030+`).  Sibling intrinsics already covered or known to be in the
same boat:

| Intrinsic | gfx950 behaviour |
| --- | --- |
| `llvm.amdgcn.permlane16` | ICE (covered by c003) |
| `llvm.amdgcn.permlanex16` | ICE (mentioned in c003) |
| `llvm.amdgcn.mov.dpp8` | ICE (this entry) |
| `llvm.amdgcn.permlane64` | OK -- gfx950 has `v_permlane64_b32` natively |
| `llvm.amdgcn.permlane16.swap`, `.permlane32.swap` | OK (`permlane16-swap` feature unset for gfx950 but the matcher handles it cleanly) |
| `llvm.amdgcn.ds.swizzle`, `ds.bpermute.fi.b32` | OK |

The proper fix is to add a target-feature predicate to the intrinsic table
so that clang either rejects the builtin at the front end or the SDAG/GISel
matcher emits a clean diagnostic instead of `report_fatal_error`.

## Fuzzer Suppression

Not yet wired up.  Add a `c004`-style suppressor in
`fuzzer/llvm_amdgpu_diff_fuzzer.cpp` to drop `Intrinsic::amdgcn_mov_dpp8`
from any IR generator that targets CDNA, mirroring c001/c002/c003.
