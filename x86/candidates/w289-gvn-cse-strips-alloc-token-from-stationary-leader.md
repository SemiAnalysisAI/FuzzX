# w289: GVN CSE strips `!alloc_token` from the stationary leader

**Severity:** Missed optimization / lost metadata-derived facts. Loss of
allocator-typed-allocation tagging used by type-aware allocator passes.

**Where:**
- `llvm/lib/Transforms/Utils/Local.cpp:3050-3056` (`MD_alloc_token` arm)
- `llvm/lib/Transforms/Scalar/GVN.cpp:2790` (`patchAndReplaceAllUsesWith`)

(file paths under `/home/orenamd@semianalysis.com/FuzzX/amdgpu/third_party/llvm-project/`)

## Root cause

The `MD_alloc_token` arm of `combineMetadata` runs unconditionally
(no `!AAOnly` guard, no `DoesKMove` guard):

```cpp
3050:      case LLVMContext::MD_alloc_token:
3051:        // Preserve !alloc_token if both K and J have it, and they are equal.
3052:        if (KMD == JMD)
3053:          K->setMetadata(Kind, JMD);
3054:        else
3055:          K->setMetadata(Kind, nullptr);
3056:        break;
```

When K has `!alloc_token` and J does not, `KMD != JMD` (token vs null), and
the `else` branch wipes K's `!alloc_token`. In GVN CSE's
stationary-leader case (`DoesKMove=false`), K's call site is unchanged
and the source explicitly annotated this allocation site with its
allocation type — the annotation is now lost.

Same shape as w287 (`!nontemporal`) and w288 (`!nosanitize`): a metadata
arm strips K's annotation when J lacks it, ignoring that K is not moving.

## Reproducer

```ll
; opt -passes=gvn -S
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

declare ptr @alloc() memory(none)

define ptr @f() {
  %a = call ptr @alloc(), !alloc_token !0   ; tagged with type info
  %b = call ptr @alloc()                    ; untagged
  ret ptr %a
}

!0 = !{!"struct.S", i64 32}
```

`opt -passes=gvn -S`:

```ll
define ptr @f() {
  %a = call ptr @alloc()   ; <-- !alloc_token gone
  ret ptr %a
}
```

Type-aware allocator instrumentation (e.g. `-fsanitize=alloc-token`,
`-falloc-token-mode=…`) running after GVN no longer sees the type tag, so
the heap is partitioned only generically; the type-isolation security
property the annotation was meant to enforce is silently weakened.

## Suggested fix

```cpp
case LLVMContext::MD_alloc_token:
  // Preserve K's !alloc_token when K is stationary. When K moves, only
  // keep it if K and J had identical tokens.
  if (DoesKMove) {
    if (KMD == JMD)
      K->setMetadata(Kind, JMD);
    else
      K->setMetadata(Kind, nullptr);
  }
  // else: K's alloc_token is correct as-is
  break;
```

## Default x86 -O2 only

Reproduces with `opt -passes=gvn -S` (and `opt -O2 -S` with
`-fsanitize=alloc-token` lowering deferred to after GVN) on
`x86_64-unknown-linux-gnu`. No source-level changes required.
