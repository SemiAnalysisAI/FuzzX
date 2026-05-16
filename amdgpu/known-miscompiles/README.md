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

Repeated input values can be written as `value*count`, for example
`0x0*129`. Intermittent testcases can add `; RUN-REPEAT: N`, or pass the repeat
count as the fourth argument; in repeat mode the runner stops at the first
observed O0/O2 mismatch.

Some testcases add `; RUN-COMBINED: 1`. For those, the runner compiles the same
IR into separate `fuzz_kernel_o0` and `fuzz_kernel_o2` objects, links both into
one code object, and runs both kernels from that combined object.

The runner defaults to `/opt/rocm-7.1.1`, `gfx950`, and device `0`. Override
those with `ROCM_PATH`, `MCPU`, and the script's third argument respectively.
If a testcase has a `; RUN-LLVM-BUILD:` comment and `CLANG` / `LLD` are not set,
the runner uses that build directory's `bin/clang` and `bin/lld`.
