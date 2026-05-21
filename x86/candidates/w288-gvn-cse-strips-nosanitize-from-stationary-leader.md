# w288: GVN CSE strips `!nosanitize` from the stationary leader

**Severity:** Correctness-adjacent for sanitizer builds. Causes spurious
sanitizer reports / instrumentation regressions; potentially miscompile in
hand-written ASan/MSan-aware code.

**Where:**
- `llvm/lib/Transforms/Utils/Local.cpp:3040-3043` (`MD_nosanitize` arm)
- `llvm/lib/Transforms/Scalar/GVN.cpp:2790` and 1158, 1625 (`combineMetadataForCSE` call sites)

(file paths under `/home/orenamd@semianalysis.com/FuzzX/amdgpu/third_party/llvm-project/`)

## Root cause

The `MD_nosanitize` arm of `combineMetadata` runs unconditionally
(no `!AAOnly` guard, no `DoesKMove` guard):

```cpp
3040:      case LLVMContext::MD_nosanitize:
3041:        // Preserve !nosanitize if both K and J have it.
3042:        K->setMetadata(Kind, JMD);
3043:        break;
```

When invoked from GVN's CSE path
(`patchAndReplaceAllUsesWith` → `patchReplacementInstruction` →
`combineMetadataForCSE(K=Repl, J=I, false)`), the iteration over K's
metadata visits `MD_nosanitize` because K has it. `JMD` is fetched as
`J->getMetadata(MD_nosanitize)`; if J wasn't marked nosanitize, JMD is
nullptr, and `K->setMetadata(Kind, nullptr)` **strips K's nosanitize**.

This is wrong for `DoesKMove=false`: K is stationary, K's load executes
exactly as written, and the source code (typically a runtime helper /
allocator probe / TLS-base read) explicitly asked the sanitizer to skip
this access. CSE-eliminating an unrelated identical load J should not
re-arm the sanitizer on K.

The same pattern affected `MD_nontemporal` (see w287). This case is more
serious because `!nosanitize` is a correctness contract with downstream
sanitizer passes (ASan, TSan, MSan, HWASan), not just a hint.

## Reproducer

```ll
; opt -passes=gvn -S
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

define i32 @f(ptr %p) {
  %a = load i32, ptr %p, align 4, !nosanitize !0   ; runtime/probe load
  %b = load i32, ptr %p, align 4                    ; ordinary user load
  %c = add i32 %a, %b
  ret i32 %c
}

!0 = !{}
```

`opt -passes=gvn -S`:

```ll
define i32 @f(ptr %p) {
  %a = load i32, ptr %p, align 4   ; <-- !nosanitize gone
  %c = add i32 %a, %a
  ret i32 %c
}
```

If this IR is fed to `-passes='gvn,asan'` (or any pipeline where GVN runs
before sanitizer instrumentation), the surviving load is now instrumented.
A runtime that loaded a sentinel pointer relying on `!nosanitize` to skip
shadow-memory checks will now trip a false report.

Codegen consequence under `-fsanitize=address` with `gvn` running before
the ASan instrumentation pass: surviving `%a` gets shadow-checked despite
the source intent.

## Why CSE-without-move shouldn't strip nosanitize

The intent of `!nosanitize` is "this access at this site should not be
sanitized". K's site doesn't change in CSE-without-move. K's metadata
should be preserved verbatim. J's absence-of-nosanitize is irrelevant; J
isn't going to execute (it's being deleted).

Analogous correctly-guarded cases for stationary K:
- `MD_invariant_load` (Local.cpp:2984-2989): empty body under
  `DoesKMove=false`, preserves K — correct.
- `MD_invariant_group` (line 2994-2996): empty body, preserves K
  — correct.
- `MD_nosanitize` (line 3040-3043): unconditional rewrite, **strips K**
  — wrong.

## Suggested fix

```cpp
case LLVMContext::MD_nosanitize:
  // Preserve K's !nosanitize when K is stationary. When K moves, only
  // keep it if both K and J had it.
  if (DoesKMove)
    K->setMetadata(Kind, JMD);
  // else: K's nosanitize is correct as-is
  break;
```

## Default x86 -O2 only

Reproduces with `opt -passes=gvn -S` and `opt -O2 -S` on
`x86_64-unknown-linux-gnu`. No source-level changes required.
