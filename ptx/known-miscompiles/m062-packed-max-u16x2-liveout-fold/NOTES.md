# m062: `max.u16x2` live-out fold corrupts later values

Found while fuzzing CUDA 13.0.88 `ptxas` after enabling broader uniform
global-load coverage. The reduced testcase is still structurally large because
the failure depends on surrounding live ranges, but the trigger is localized:
replacing the single `max.u16x2 %r0, %r5, %r1` in `reduced.ptx` with a neutral
`mov.u32 %r0, %r0` makes `-O0` and `-O3` match.

```bash
target/release/fuzzx-diff-test \
  known-miscompiles/m062-packed-max-u16x2-liveout-fold/reduced.ptx \
  known-miscompiles/m062-packed-max-u16x2-liveout-fold/input.bin
```

Observed result:

```text
DIVERGES (deterministic) - 32/32 tids differ, 64/128 u32 slots differ
```

## Reduction Notes

The valid guarded reducer shrinks the original fuzzer program from 1142 lines to
935 lines. The remaining mismatch appears in output slots written by global
store/load roundtrip scaffolding, not the final scalar output store:

```text
O0: [00000057 00008877 04000000 00001405]
O3: [00000057 000002aa 04000000 00000000]
```

Manual cuts show:

- Cutting immediately after the earlier `ldu.global.v4.b32` has matching
  outputs, so the new uniform vector load is not the root cause.
- Cutting after the packed-min block diverges.
- Replacing only `max.u16x2` with `mov.u32` removes the divergence.

## Fuzzer Follow-Up

The post-known suppression profile now disables packed min/max generation. To
re-enable this class explicitly, omit `--disable-packed-minmax`.
