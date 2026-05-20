# c007: `llvm.amdgcn.fcmp.i32` with two equal constant operands ICEs at `-O2` on wave64 targets

The `llvm.amdgcn.fcmp` intrinsic returns an i32-or-i64 wave mask (per its
definition in `IntrinsicsAMDGPU.td`).  The size the user asks for is
expected to match the target's wave size.  When it doesn't -- e.g. asking
for `i32` on a wave64 target -- the SDAG combiner does not validate this
before constant-folding the FP compare.  At `-O2`, a fold of `fcmp(0.0,
0.0, OEQ)` to all-true triggers ISel, which tries to materialize the
folded mask as `exec` and fails the register-class check.

```bash
known-miscompiles/run_ll_compiler_reproducer.sh \
  known-miscompiles/c007-fcmp-i32-wave64-fold-ice/reduced.ll
```

Observed output (LLVM HEAD with the five PR patches, gfx950):

```text
O0=pass
O2=fail
O2-exit=1
O2-message=fatal error: error in backend: invalid type for register "exec".
compiler_failure=true
```

`-O0` succeeds (no constant folding), and `-O2` with non-equal operands also
succeeds (no fold).  Wave32 targets like `gfx1030` also succeed for the
`i32` form (because the wave size matches the return type) -- but the
mirror ICEs reliably on **any** wave64 target, e.g.:

```bash
clang -O2 -target amdgcn-amd-amdhsa -mcpu=gfx1030 -mwavefrontsize64 \
  -S c007-fcmp-i32-wave64-fold-ice/reduced.ll -o /dev/null
# -> fatal error: error in backend: invalid type for register "exec".
```

## Distinct From c003--c006

`c003`--`c006` are all "intrinsic not supported on this subtarget" cases
where the backend should produce a clean "intrinsic not supported on
subtarget" diagnostic.  `c007` is different: the intrinsic *is* available
on wave64 targets, just with a different return type; the bug is in the
constant folder, which can be silently producing wrong code at `-O0`
(uninvestigated) and reliably crashes when the fold fires at `-O2`.

A defensible fix would be to teach the `fcmp`/`icmp` intrinsic lowering to
either (a) issue a verifier error when the requested integer width does
not match `Subtarget.getWavefrontSize()`, or (b) zero-extend / truncate the
wave mask to the requested width after selection so the register class
matches.

## Fuzzer Suppression

Not yet wired up.  Add a `c007`-style suppressor to disable
`Intrinsic::amdgcn_fcmp` (and `amdgcn_icmp`) calls whose return type does
not match the active wave size.
