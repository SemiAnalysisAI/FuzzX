# w105 - SimplifyCFG hoistCommonCodeFromSuccessors silently drops `!nontemporal` on memcpy/memmove/memset

## Location

`llvm/lib/Transforms/Utils/SimplifyCFG.cpp` -
`SimplifyCFGOpt::hoistCommonCodeFromSuccessors`. The instruction-identity
check (`isSameOperationAs`) does not look at metadata, so two memory
intrinsic calls whose operands and attributes match are deemed equivalent
even when only one of them carries `!nontemporal`. The merge then runs
`combineMetadataForCSE` (`llvm/lib/Transforms/Utils/Local.cpp:2934-3057`),
which for `MD_nontemporal` does:

```
case LLVMContext::MD_nontemporal:
  // Preserve !nontemporal if it is present on both instructions.
  if (!AAOnly)
    K->setMetadata(Kind, JMD);
  break;
```

When `J` (the sibling-block instruction) is missing the metadata, `JMD` is
null, so the metadata is unconditionally cleared on the hoisted call. The
nontemporal-bearing program point is silently lost.

This mirrors w76 (memcpyopt-trymerge-drops-nontemporal) and w75 (DSE), and
parallels the SimplifyCFG family already filed (w120/w121/w57). The same
issue applies to `memcpy`, `memmove` and `memset`.

## Repro

```llvm
target triple = "x86_64-unknown-linux-gnu"

define void @hoist_memcpy(i1 %c, ptr %d, ptr %s) {
entry:
  br i1 %c, label %then, label %else
then:
  call void @llvm.memcpy.p0.p0.i64(ptr %d, ptr %s, i64 64, i1 false), !nontemporal !0
  br label %tail
else:
  call void @llvm.memcpy.p0.p0.i64(ptr %d, ptr %s, i64 64, i1 false)
  br label %tail
tail:
  ret void
}

declare void @llvm.memcpy.p0.p0.i64(ptr, ptr, i64, i1)

!0 = !{ i32 1 }
```

## Invocation

```
opt -passes='simplifycfg<hoist-common-insts>' -S input.ll
```

## Before / after

Before:
- in `then` the memcpy carries `!nontemporal`, meaning the hardware is asked
  to bypass the cache;
- in `else` the memcpy is a normal cacheable copy.

After (observed):

```
entry:
  call void @llvm.memcpy.p0.p0.i64(ptr %d, ptr %s, i64 64, i1 false)
  ret void
```

The two calls are hoisted into a single call and `!nontemporal` is dropped
entirely. For the `then` path the IR now emits a normal cached copy, losing
the program's explicit cache-bypass hint. Downstream codegen lowers a normal
memcpy (e.g. `rep movsq` or vectorized stores) instead of the streaming
nontemporal variant (`movntdq`/`movntps`), so the observable cache footprint
changes for one of the two original paths.

While `!nontemporal` is officially a hint, dropping it across a join that
contains a conditional decision is an asymmetric loss: only the `then`-path
program suffers, and there is no way for the front-end to recover the hint
after SimplifyCFG. This is identical in spirit to the volatile/atomic merges
already filed - SimplifyCFG must compare full memory-effect-bearing metadata
before deciding two instructions are equivalent.

## Recommended fix

`hoistCommonCodeFromSuccessors` should treat `!nontemporal` (and the related
`MD_nontemporal` group) as part of the operation identity, refusing to hoist
two memory ops if one carries it and the other does not. Either reuse the
existing MMRA equality check pattern (line 1954) by extending it to
`MD_nontemporal`, or guard `combineMetadataForCSE` with an early return when
the metadata asymmetry would silently drop the hint.
