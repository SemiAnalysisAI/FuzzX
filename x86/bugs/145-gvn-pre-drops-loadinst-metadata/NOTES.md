# w83 GVN: `eliminatePartiallyRedundantLoad` drops correctness-relevant load metadata when inserting the PRE'd load

## Location
- `llvm/lib/Transforms/Scalar/GVN.cpp:1565-1633` (`GVNPass::eliminatePartiallyRedundantLoad`)

The PRE inserter only forwards a fixed set of metadata from the original
`Load` onto the newly-created predecessor load:

```cpp
auto *NewLoad = new LoadInst(
    Load->getType(), LoadPtr, Load->getName() + ".pre", Load->isVolatile(),
    Load->getAlign(), Load->getOrdering(), Load->getSyncScopeID(),
    UnavailableBlock->getTerminator()->getIterator());
...
NewLoad->setAAMetadata(Load->getAAMetadata());
if (auto *MD = Load->getMetadata(MD_invariant_load))     NewLoad->setMetadata(...);
if (auto *MD = Load->getMetadata(MD_invariant_group))    NewLoad->setMetadata(...);
if (auto *MD = Load->getMetadata(MD_range))              NewLoad->setMetadata(...);
if (auto *MD = Load->getMetadata(MD_nofpclass))          NewLoad->setMetadata(...);
if (auto *MD = Load->getMetadata(MD_access_group))       NewLoad->setMetadata(...);
```

`MD_noundef`, `MD_align`, `MD_dereferenceable`, `MD_dereferenceable_or_null`,
`MD_nonnull`, `MD_nontemporal`, and `MD_alias_scope` are **silently dropped**
on the inserted load. The original load is then replaced by the PHI of
`%v.pre` and existing-pred values, so the metadata is lost from the IR
entirely.

## Reproducer (alignment hint dropped, but kept as a hint)

```ll
define i32 @pre_align_md(ptr %p, i1 %c, ptr %val) {
entry:
  br i1 %c, label %then, label %else
then:
  store ptr %val, ptr %p, align 8
  br label %join
else:
  br label %join
join:
  %loaded = load ptr, ptr %p, align 8, !align !0
  %v = load i32, ptr %loaded, align 16
  ret i32 %v
}
!0 = !{i64 16}
```

`opt -passes=gvn -S` produces:

```ll
else:
  %loaded.pre = load ptr, ptr %p, align 8        ; <-- !align !0 dropped
  br label %join

join:
  %loaded = phi ptr [ %loaded.pre, %else ], [ %val, %then ]
  %v = load i32, ptr %loaded, align 16           ; align 16 no longer backed by metadata
  ret i32 %v
```

## Severity caveat

For `!align`/`!nonnull`/`!dereferenceable`/`!nontemporal`/`!noundef` the
drop is a *weakening* (an optimizer hint or UB-trap is lost) and is
strictly correct - downstream code that already uses the value is at
worst slower, not wrong. But the bug is real: GVN is supposed to combine
metadata using `combineMetadataForCSE`/`copyMetadataForLoad`, and the
manually-coded subset here misses everything that was added after the
function was written. New metadata kinds (e.g. `MD_noalias_addrspace`)
keep regressing through this path.

## Suggested fix

Replace the bespoke list with `copyMetadataForLoad(*NewLoad, *Load)` plus
a follow-up `combineMetadataForCSE(...)` against the kept available value
(which is what `processLoad` does on the local path). That centralizes
metadata propagation policy.

## opt diff summary

Hand-verified via:
- `/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt -passes=gvn -S /tmp/w83_pre_align_md.ll`
- `/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt -passes=gvn -S /tmp/w83_final_test.ll` (nontemporal drop)
