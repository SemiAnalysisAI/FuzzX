# FuzzX PTX Design

## Goal

Find `ptxas` bugs in generated PTX programs, with the current emphasis on
miscompiles: cases where two optimization levels compile and launch
successfully but produce different outputs for the same input.

## Architecture

```text
seed
  |
  v
deterministic bytes
  |
  v
fuzzx-execgen -> PTX kernel + input buffer
  |
  +--> ptxas -O0 -> cubin -> CUDA launch -> output_o0
  |
  +--> ptxas -O3 -> cubin -> CUDA launch -> output_o3
                                      |
                                      v
                              compare outputs
                                      |
                                      v
                          divergences/div-.../
```

`fuzzx-diff` is intentionally undirected. It does not use coverage feedback;
it walks a deterministic seed stream and relies on throughput plus generator
diversity.

Each saved divergence bundle contains enough state to inspect, verify, and
reduce the candidate: generated seed bytes, PTX, runtime input, both outputs or
errors, and a short summary.

## Generator Invariants

`fuzzx-execgen` emits kernels with a fixed ABI:

```text
.visible .entry fuzz_kernel(.param .u64 in, .param .u64 out, .param .u32 n)
```

The generator keeps mismatches meaningful by enforcing these invariants:

- Working registers are initialized before use.
- Loop backedges are bounded by countdown registers.
- Each thread writes to its own output slice.
- There is no shared memory, atomics, warp communication, or barriers.
- Integer operations are preferred so differences are not explained by floating
  point behavior.

## Verification And Reduction

`fuzzx-diff-verify` reruns a saved divergence and checks that the mismatch is
still present.

`fuzzx-diff-reduce` greedily removes PTX lines while preserving these
conditions:

- both `-O0` and `-O3` compile;
- both cubins launch successfully;
- each optimization level is deterministic across repeated launches;
- the two optimization levels still disagree.

The reducer keeps the prologue and address arithmetic intact so reductions do
not introduce output races or out-of-bounds memory access.

`fuzzx-diff-test`, `fuzzx-diff-sweep`, `fuzzx-diff-dump-gen`, and
`fuzzx-diff-inspect-outputs` are interactive helpers for manual reduction and
inspection.

## Constraints

- The differential fuzzer currently targets the architecture named by
  `fuzzx-execgen::TARGET_ARCH` (`sm_103` by default).
- CUDA contexts are thread-local; workers own their CUDA context and reusable
  buffers.
- `ptxas` temp files are routed through `TMPDIR`; the differential tools prefer
  `/dev/shm` when the caller has not set `TMPDIR`.
