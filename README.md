# FuzzX

FuzzX is a collection of compiler fuzzers.

| Directory | Purpose |
| --- | --- |
| [`ptx/`](ptx/) | NVIDIA `ptxas` fuzzer |
| [`amdgpu/`](amdgpu/) | AMDGPU fuzzer |
| [`x86/`](x86/) | x86 fuzzer |
| [`spirv/`](spirv/) | LLVM SPIR-V backend crash fuzzer |

See each subdirectory for build and run instructions.

Some of these bugs were found by fuzzing, but I also tried simply asking Claude
to find bugs in the AMDGPU and x86 LLVM backends.

This repository is mostly for demonstration purposes, it's the result of an
experiment in vibe coding.  Don't rely on anything here.

In particular, you should not make any inferences about the quality of a
particular compiler based on the data here.  If a compiler has lots of bugs
listed, many of them could have the same root cause, or indeed they might not
be bugs at all, I haven't verified most of this myself.  On the other hand if a
compiler has relatively few bugs, they might all have different root causes,
and/or I might have spent relatively little effort tryin to find bugs in that
compiler.
