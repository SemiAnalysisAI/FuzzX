# 071 — `opt -passes=codegenprepare` segfaults on any IR (missing PSI/BFI analyses)

Component: CodeGen/CodeGenPrepare + opt pass pipeline glue

## Symptom

```
$ cat repro.ll
define void @f() { ret void }
$ opt -passes=codegenprepare -mtriple=x86_64-linux-gnu -S repro.ll
PLEASE submit a bug report to https://github.com/llvm/llvm-project/issues/ ...
0  opt   ... llvm::sys::PrintStackTrace
3  libc  ... (SIGSEGV)
4  opt   ... llvm::ProfileSummaryInfo::isFunctionHotInCallGraph<...>
5  opt   ... CodeGenPrepare::_run
6  opt   ... llvm::CodeGenPreparePass::run
$ echo $?
139
```

Crash address resolves (via `addr2line`) to:

```
_ZNK4llvm18ProfileSummaryInfo24isFunctionHotInCallGraphINS_8FunctionENS_18BlockFrequencyInfoEEEbPKT_RT0_
CodeGenPrepare.cpp:?
_ZN12_GLOBAL__N_114CodeGenPrepare4_runERN4llvm8FunctionE
```

The `CodeGenPreparePass` (new-PM) calls into `ProfileSummaryInfo::isFunctionHotInCallGraph`
with a pointer that ends up `nullptr` (or otherwise unusable) when the pass is
invoked through `opt -passes=codegenprepare`. The pass requires `ProfileSummaryAnalysis`
and `BlockFrequencyAnalysis` to be available; opt's `-passes` parser doesn't
add those preconditions, so the pass crashes on the first function it sees.

The same `codegenprepare` invocation through `llc -stop-after=codegenprepare`
works fine, because llc's pipeline builds the required analyses before
scheduling the pass.

## Why this matters

`opt -passes=codegenprepare` is the only way users can test or fuzz CGP
in isolation. It is the documented invocation in many LLVM CGP unit tests
(`opt -passes=codegenprepare ...`). The segfault is reproducible with any
IR, including an empty function — i.e., the pass cannot be run via opt at
all. That is either:
- a missing pre-analysis declaration in `CodeGenPreparePass::run` (so the
  PM materializes PSI/BFI before invoking it), or
- a missing null-check in the call to `PSI->isFunctionHotInCallGraph(...)`.

Either way it's a real null-deref crash bug under any user input.

## Reproducer

`repro.ll` is two lines (`define void @f() { ret void }`); `cmd.sh` runs
the failing invocation and checks the exit status is 139 (SIGSEGV).

## Fix sketch

In `CodeGenPreparePass::run`, guard the PSI lookup:

```cpp
ProfileSummaryInfo *PSI =
    MAMProxy.cachedResult<ProfileSummaryAnalysis>(*F.getParent());
// ... use PSI only when non-null
```

or declare a `required<ProfileSummaryAnalysis>` so the PM materializes it.
