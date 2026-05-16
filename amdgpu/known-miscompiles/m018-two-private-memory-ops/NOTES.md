# m018: ROCm 7.2.3 `-O0` has an intermittent scratch-store/load hazard

This was found while fuzzing the ROCm 7.2.3 source build with release failures
`m001`, `m013`, and `m017` suppressed.

The reproducer needs multiple work-items and repeats because the bad result is
intermittent:

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m018-two-private-memory-ops/reduced.ll
```

One observed failing run:

```text
iteration=6
index=128
input=0x0ccf4d8b
O0=0x37e05cc6
O2=0x37e05e44
mismatch=true
```

## Reduction

The reduced IR keeps two private-memory `alloca` sequences in one kernel. The
second sequence stores a value that should be independent of the input because
the value flowing from the first sequence is multiplied by zero before the
second private-memory sequence.

At `-O2`, the private memory is optimized away and the output is the expected
constant `0x37e05e44`.

At `-O0`, ROCm 7.2.3 sometimes returns an input-dependent value from the first
private-memory sequence instead.

## Root Cause Notes

The ROCm 7.2.3 `-O0` assembly for the reduced case emits scratch stores for the
second private-memory object and then immediately emits a scratch load from that
object without a visible wait between the stores and the load. The failure is
intermittent and appears in groups of work-items, which is consistent with a
scratch-memory ordering hazard in the unoptimized lowering.

The `-O2` assembly does not use scratch memory for the reduced sequence, so it
does not hit the hazard.

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| ROCm 7.2.3 source build from tag `rocm-7.2.3`, commit `f58b06dce1f9c15707c5f808fd002e18c2accf7e`, `Release`, sanitizer coverage, no ASan | Reproduces. |
| LLVM HEAD, commit `10756d32f96154f0889eda159ea9a26bc4188bda` | Passes 50 repeated combined runs. |
| ROCm HEAD, commit `9115c466b3577830455f70c4f492429bf6c64b25` | Passes 50 repeated combined runs. |

## Fuzzer Follow-Up

The directed fuzzer now suppresses programs with two private-memory operations
by default. Set `FUZZX_ALLOW_M018_TWO_PRIVATE_MEMORY_OPS=1` to re-enable this
shape.
