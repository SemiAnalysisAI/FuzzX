# 229 — GVN/EarlyCSE `combineMetadataForCSE` strips `!nontemporal` from the stationary leader when CSE'd-against load lacks it

Component: `llvm/lib/Transforms/Utils/Local.cpp` lines ~3030-3034 (MD_nontemporal arm of `combineMetadata`); called from GVN.cpp:2790 and EarlyCSE.cpp:1617.

The `MD_nontemporal` arm unconditionally writes `K->setMetadata(MD_nontemporal, JMD)`. When K is the stationary CSE leader and J (the eliminated duplicate) has no `!nontemporal`, JMD is nullptr → `setMetadata` removes the tag, silently stripping the surviving load's NT hint.

This is the wrong direction for CSE: K should keep its own annotation when it didn't move. The `MD_invariant_load` and `MD_invariant_group` arms already get this right with explicit `if (DoesKMove)` guards; the `MD_nontemporal`, `MD_nosanitize`, `MD_alloc_token` arms are buggy.

## Reproducer

`opt -passes=gvn -S repro.ll` (also `opt -passes='early-cse<memssa>' -S` per w580 verification):

Before: `%a = load i32, ptr %p, align 4, !nontemporal !0`. After: `%a = load i32, ptr %p, align 4` — `!nontemporal` silently dropped.

## Severity

Default x86 -O2. NT loads get downgraded to plain MOV whenever a sibling load to the same address lacks NT (e.g., from instrumentation or other passes that introduce a plain duplicate).

## Fix

In `combineMetadata` `MD_nontemporal` arm, guard with `if (DoesKMove)` — only allow stripping when K was actually moved.
