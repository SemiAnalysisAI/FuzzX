# Known ptxas Miscompiles

Each subdirectory is a self-contained record for one confirmed `ptxas`
miscompile or optimizer crash. The PTX `README.md` has the bug table.

## Standard Files

Every current bug directory has these files:

| File | What it is |
| --- | --- |
| `reduced.ptx` | Minimized PTX testcase. |
| `repro_nvcc_inline_ptx.cu` | CUDA inline-PTX standalone reproducer for the reduced testcase. |
| `NOTES.md` | Per-bug analysis, checked toolchain versions, and root-cause notes. |

Some older directories may also have archival fuzzer artifacts such as
`program.ptx` or `summary.txt`. Those files are not part of the current
standalone reproducer format and are not required.

## Standalone Reproducers

The current standalone reproducers are CUDA `.cu` files with inline PTX. Build
the same source twice and compare the printed output:

```bash
nvcc -std=c++17 -O2 -arch=sm_103 -Xptxas -O0 \
  repro_nvcc_inline_ptx.cu -o repro_nvcc_inline_ptx_o0

nvcc -std=c++17 -O2 -arch=sm_103 -Xptxas -O2 \
  repro_nvcc_inline_ptx.cu -o repro_nvcc_inline_ptx_o2

./repro_nvcc_inline_ptx_o0
./repro_nvcc_inline_ptx_o2
```

For `m023-mul-wide-hi-ice`, the `-Xptxas -O2` build itself is the reproducer:
it is expected to fail in optimized `ptxas` before a runnable binary is
produced.

Each bug's `NOTES.md` records the `nvcc`/`ptxas` version used to check that
standalone reproducer.
