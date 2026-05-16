# Known AMDGPU Miscompiles

Each subdirectory is a self-contained record for one confirmed AMDGPU compiler
miscompile or optimizer crash. The AMDGPU `README.md` has the bug table.

## Standard Files

Every current bug directory has these files:

| File | What it is |
| --- | --- |
| `reduced.ll` | Minimized LLVM IR testcase, including a `; RUN-INPUTS:` comment with the input values that reproduce the mismatch. |
| `NOTES.md` | Per-bug analysis, checked toolchain versions, and root-cause notes. |

## Standalone Reproducers

Use the generic runner script to compile the same LLVM IR at `-O0` and `-O2`,
run both code objects through HIP, and print the observed output words:

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m001-ashr-i16-zext/reduced.ll
```

If the second argument is omitted, the runner reads input values from the first
`; RUN-INPUTS:` comment in the `.ll` file. Override them by passing a comma- or
space-separated input list as the second argument:

```bash
known-miscompiles/run_ll_reproducer.sh \
  known-miscompiles/m001-ashr-i16-zext/reduced.ll \
  "0x7fffffff,0x00008000"
```

The runner defaults to `/opt/rocm-7.1.1`, `gfx950`, and device `0`. Override
those with `ROCM_PATH`, `MCPU`, and the script's third argument respectively.
If a testcase has a `; RUN-LLVM-BUILD:` comment and `CLANG` / `LLD` are not set,
the runner uses that build directory's `bin/clang` and `bin/lld`.
