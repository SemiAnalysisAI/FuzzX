# 220 — `patchReplacementInstruction` drops `nsw`/`nuw` on the kept dominator, clobbering pre-existing shared users

Component: `llvm/lib/Transforms/Utils/Local.cpp` lines ~3190-3192 (`patchReplacementInstruction`)

When GVN CSEs `extractvalue 0` of `llvm.s/uadd.with.overflow` against a dominating `add nsw/nuw`, it calls `ReplInst->dropPoisonGeneratingFlags()` on the kept value. The drop is **global to ReplInst** — pre-existing users of ReplInst that legitimately relied on `nsw`/`nuw` silently lose those flags.

## Reproducer

`opt -passes=gvn -S repro.ll`:

Input has `%addnsw = add nsw i32 %x, %y`. After GVN, the kept add becomes `%addnsw = add i32 %x, %y` (no nsw), AND the `call void @use(i32 %res)` becomes `call void @use(i32 %addnsw)`. So the pre-existing first `call void @use(%addnsw)` (which previously consumed an `add nsw`) now consumes a plain `add` — a fact that was valid at that site is silently lost.

## Severity

Default x86 -O2. Loss of optimization information that propagates through downstream KnownBits/SCEV/range analyses. Subtle because the loss isn't at the CSE site but at unrelated users.

## Fix

Only drop the flags if there are no other (non-CSE) users that benefit from them, or split the CSE'd value so the dominator's flags aren't disturbed.
