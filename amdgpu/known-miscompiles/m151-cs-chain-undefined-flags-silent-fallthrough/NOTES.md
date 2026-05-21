# m151: `SITargetLowering::LowerCall` silently accepts undefined `llvm.amdgcn.cs.chain` Flags values

*Discovery method: code inspection (during chain-call / SI_TCRETURN_CHAIN audit).*

Sibling defect to m145 (`MO_ExternalSymbol` drops target-flag specifier)
-- both in the chain-call lowering family in `SITargetLowering::LowerCall`.

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/SIISelLowering.cpp:4248-4266`
(chain-call special-argument dispatch):

```cpp
if (FlagsValue.isZero()) {
  if (CLI.Args.size() > 5)
    fail("...");                 // (1) flags == 0, no extra args
} else if (FlagsValue.isOneBitSet(0)) {
  // (2) flags == 1, DVGPR path; validate 3 extra args
  ...
  ChainCallSpecialArgs.push_back(NumVGPRs);
  ChainCallSpecialArgs.push_back(FallbackExec);
  ChainCallSpecialArgs.push_back(FallbackCallee);
}
// no else clause
```

There is **no else** clause invoking `lowerUnhandledCall("invalid flags")`
or otherwise diagnosing.  Any `Flags` value that is neither `0` nor
exactly `1` (e.g. `2`, `3`, `5`, ...) falls through silently:

* `UsesDynamicVGPRs` stays `false` (`SIISelLowering.cpp:4208`).
* `ChainCallSpecialArgs` only contains `exec` -- `NumVGPRs`,
  `FallbackExec`, `FallbackCallee` are never pushed (the loop at
  line 4264 is skipped).
* The opcode picked at `SIISelLowering.cpp:4602` is
  `AMDGPUISD::TC_RETURN_CHAIN`, selecting the non-DVGPR
  `SI_CS_CHAIN_TC_W32`/`_W64` pseudo (`SIInstructions.td:900-901`)
  instead of `_DVGPR` (`:905-907`).
* `CLI.Args` still contains the trailing IR-level variadic args
  (NumVGPRs / FallbackExec / FallbackCallee).  They are dropped
  from the lowered call without diagnostic.

Result: caller-visible loss of fallback semantics for an `immarg`
value the user supplied.  The user wrote `flags = 2` with intent
that the implementation accept (some-future-meaning), but the
compiler silently runs the non-DVGPR path and discards their
trailing args.

## Reachability

* SDAG, all AMDGPU targets that support `amdgcn.cs.chain` (wave32
  primarily).
* The IR Verifier (`llvm/lib/IR/Verifier.cpp:7061-7092`) does NOT
  range-check the `Flags` immarg; the only validation lives in
  this `LowerCall` block, and it has the silent-fallthrough hole.
* No upstream lit test exercises `flags` values other than 0/1
  (grep for the existing error strings under
  `llvm/test/CodeGen/AMDGPU/` returns nothing).

## Reproducer

`reduced.ll` calls `llvm.amdgcn.cs.chain` with `flags = 2` and the
4 fallback args.  The lowering silently:
* Selects the non-DVGPR pseudo.
* Drops the 4 trailing args from the emitted call.
* Does NOT emit a diagnostic.

```llvm
call void (ptr, i32, <3 x i32>, i32, i32, ...)
  @llvm.amdgcn.cs.chain.p0.i32.v3i32.i32(
      ptr @callee, i32 %exec_mask, <3 x i32> %sgpr, i32 %vgpr,
      i32 2,                ; <-- Flags = 2 (undefined; silent)
      i32 %num_vgprs, i32 %fallback_exec, i32 %fallback_callee)
```

## Suggested fix

Add an `else` branch invoking `lowerUnhandledCall("invalid flags")`:

```cpp
if (FlagsValue.isZero()) {
  ...
} else if (FlagsValue.isOneBitSet(0)) {
  ...
} else {
  return lowerUnhandledCall(CLI, InVals,
                            "invalid flags for amdgcn.cs.chain (must be 0 or 1)");
}
```

Or add a verifier check in `llvm/lib/IR/Verifier.cpp` that
range-checks the immarg.

## Why the fuzzer hasn't caught it

* The IR fuzzer rarely generates `amdgcn.cs.chain` with random
  immarg values.  Per `MEMORY.md` (Prefer-random-over-idioms), the
  random emitter should produce `cs.chain` calls with `flags` ∈
  {0, 1, 2, 3, 5, 0x7FFFFFFF} and expect either a clean diagnostic
  or correct codegen.
* The differential O0-vs-O2 oracle won't catch this because both
  opt levels share the same `LowerCall` path.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Silent fallthrough; trailing args dropped without diagnostic. |
| ROCm 7.1.1 | Same defect. |

## Family

* m145 (`MO_ExternalSymbol` drops target-flag specifier) -- same
  file, same lowering pass, same class of "argument-count vs
  emitted-pseudo arity mismatch".
* c012 (`pops.exiting.wave.id` wrong target gate) -- different
  defect class but same theme: intrinsic immarg / target gate not
  validated.
