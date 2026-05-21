# w287: GVN load CSE strips `!nontemporal` from the stationary leader

**Severity:** Missed optimization / lost hint. Performance regression on
streaming workloads that explicitly annotate cache-bypassing loads.

**Where:**
- `llvm/lib/Transforms/Scalar/GVN.cpp:2151-2154` (`patchAndReplaceAllUsesWith`)
- `llvm/lib/Transforms/Scalar/GVN.cpp:2790` (call site in `processInstruction`)
- `llvm/lib/Transforms/Utils/Local.cpp:3030-3034` (`MD_nontemporal` arm)
- `llvm/lib/Transforms/Utils/Local.cpp:3180-3221` (`patchReplacementInstruction`)

(file paths under `/home/orenamd@semianalysis.com/FuzzX/amdgpu/third_party/llvm-project/`)

## Root cause

When `processInstruction` finds that an instruction `I` (J in the CSE
algebra below) has a dominating leader `Repl` (K) with the same value
number, it calls

```cpp
2790:  patchAndReplaceAllUsesWith(I, Repl);
```

which routes through `patchReplacementInstruction(I, Repl)` →
`combineMetadataForCSE(ReplInst, I, /*DoesKMove=*/false)`
(`Local.cpp:3220`). The `MD_nontemporal` arm of `combineMetadata` is:

```cpp
3030:      case LLVMContext::MD_nontemporal:
3031:        // Preserve !nontemporal if it is present on both instructions.
3032:        if (!AAOnly)
3033:          K->setMetadata(Kind, JMD);
3034:        break;
```

This case is reached because K (the leader) has `!nontemporal`. `JMD` is
fetched as `J->getMetadata(MD_nontemporal)`. If J doesn't have
`!nontemporal`, `JMD == nullptr`, and `K->setMetadata(Kind, nullptr)`
**unconditionally strips K's `!nontemporal`** — *even though K never
moves and its access executes exactly as before*.

For `DoesKMove=true` the "both must have it" rule is defensible (K
relocates to a site that wasn't marked nontemporal, so maintaining the
nontemporal contract is questionable). For `DoesKMove=false` it is just
wrong: K's load instruction is unchanged, the user explicitly asked for a
non-temporal access at K's site, and the eliminated J was never going to
execute anyway.

## Reproducer

```ll
; opt -passes=gvn -S
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

define i32 @f(ptr %p) {
  %a = load i32, ptr %p, align 4, !nontemporal !0   ; K: stationary leader, nontemporal
  %b = load i32, ptr %p, align 4                     ; J: about to be CSE-eliminated
  %c = add i32 %a, %b
  ret i32 %c
}

!0 = !{i32 1}
```

`opt -passes=gvn -S`:

```ll
define i32 @f(ptr %p) {
  %a = load i32, ptr %p, align 4   ; <-- !nontemporal silently stripped
  %c = add i32 %a, %a
  ret i32 %c
}
```

Codegen change: x86 would have lowered `%a` to `MOVNTDQA` (or `VMOVNTDQA`
under AVX) per the nontemporal hint; after GVN, the lowering reverts to a
plain `MOV`, polluting the cache for what the source code declared as a
streaming read.

## Why this matters more for `DoesKMove=false`

`combineMetadataForCSE` has two callers in GVN.cpp:

1. `combineMetadataForCSE(NewLoad, OldLoad, false)` (line 1625, 1158) — the
   "we just created a load that subsumes both" case.
2. `patchReplacementInstruction` → `combineMetadataForCSE(Repl, I, false)`
   (line 2790 → `Local.cpp:3220`) — the "use existing leader" case.

In case (2), there is no semantic reason to weaken K's metadata: K is
exactly the instruction the source wrote, it executes exactly as written,
and J's only role is to be deleted. The "intersect with J" rule treats J's
absence-of-nontemporal as a positive assertion "this access is NOT
non-temporal" — which J never claimed; J simply omitted the hint.

The same shape applies to all other metadata arms gated on
`DoesKMove`-or-equivalent in `combineMetadata` that strip K's metadata
when J lacks it:

- `MD_invariant_load` (line 2984-2989): when K has `!invariant_load` and J
  doesn't, the case body is empty under `DoesKMove=false`, so K's
  `!invariant_load` is *preserved* — correct behavior here.
- `MD_nontemporal` (line 3030-3034): K's `!nontemporal` is *dropped*
  — **wrong**.
- `MD_noundef` (line 3025-3029): gated on `!AAOnly && DoesKMove`, so under
  `DoesKMove=false` the body is empty — correct (K's `!noundef` preserved).
- `MD_nosanitize` (line 3040-3043): `K->setMetadata(Kind, JMD)`
  unconditional — dropped if J lacks it; similar latent bug for `nosanitize`
  on a stationary leader.

## Suggested fix

```cpp
case LLVMContext::MD_nontemporal:
  // Preserve !nontemporal if it is present on both instructions, OR if
  // K is stationary (DoesKMove=false), since K's runtime behavior is
  // unchanged by CSE.
  if (!AAOnly && DoesKMove)
    K->setMetadata(Kind, JMD);
  // else: leave K's nontemporal alone
  break;
```

Apply the same `DoesKMove` guard to the `MD_nosanitize` arm.

## Default x86 -O2 only

Reproduces with `opt -passes=gvn -S` (and `opt -O2 -S`) on
`x86_64-unknown-linux-gnu`. No source-level changes required.
