# `combineMetadata`: `!invariant.group` on `K` is silently CLOBBERED with `J`'s value (any caller, any DoesKMove)

**Pass surface:** any pass that calls `combineMetadataForCSE`/`combineAAMetadata`. Confirmed for `early-cse` and `simplifycfg` below.
**Source:** `llvm/lib/Transforms/Utils/Local.cpp` lines 3059-3067:
```cpp
// Set !invariant.group from J if J has it. If both instructions have it
// then we will just pick it from J - even when they are different.
// ...
// FIXME: we should try to preserve both invariant.group md if they are
// different, but right now instruction can only have one invariant.group.
if (auto *JMD = J->getMetadata(LLVMContext::MD_invariant_group))
  if (isa<LoadInst>(K) || isa<StoreInst>(K))
    K->setMetadata(LLVMContext::MD_invariant_group, JMD);
```
**Triple:** `x86_64-unknown-linux-gnu`
**Tool:** `/home/orenamd@semianalysis.com/FuzzX/amdgpu/build/llvm-fuzzer/bin/opt -S -passes=early-cse` (or `simplifycfg`).

## Root cause

`combineMetadata` runs the per-kind switch ONLY for kinds present on `K`. After the switch, an unconditional epilogue overwrites `K->invariant.group` with `J->invariant.group` whenever `J` has it. There is no equality check, no respect for `DoesKMove`, and no respect for the `AAOnly` flag. So even in the CSE case (`DoesKMove=false`, K is the kept dominator, J is the deletion victim), the kept K silently inherits J's group tag.

Per LangRef, `!invariant.group` is a *group identifier*: stores/loads with the same tag from the same pointer must observe the same value, independent of intervening memory ops. Different tags grant a barrier (`llvm.launder.invariant.group`) the ability to "break" the equivalence. Replacing K's tag with J's silently breaks any downstream pass (or front-end intrinsic) that referenced the *original* tag.

## Reproducer A — EarlyCSE (caller: `EarlyCSE.cpp:1617`, `combineMetadataForCSE(I, &Inst, /*DoesKMove=*/false)`)

```llvm
target datalayout = "e-m:e-i64:64-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

define i32 @cse(ptr %p) {
entry:
  %0 = load i32, ptr %p, align 4, !invariant.group !0
  %1 = load i32, ptr %p, align 4, !invariant.group !1
  %add = add i32 %0, %1
  ret i32 %add
}

!0 = !{!"groupA"}
!1 = !{!"groupB"}
```

```
$ opt -S -passes=early-cse repro.ll
```

After:
```
entry:
  %0 = load i32, ptr %p, align 4, !invariant.group !0
  %add = add i32 %0, %0
  ret i32 %add
}
!0 = !{!"groupB"}
```

`%0` was the surviving dominator load tagged `!"groupA"`; post-CSE it is tagged `!"groupB"` (J's tag). No call to `launder.invariant.group` separated the two loads — there is no legitimate basis to swap groups.

## Reproducer B — SimplifyCFG hoist (`SimplifyCFG.cpp:2018`, DoesKMove=true)

```llvm
target datalayout = "e-m:e-i64:64-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

define i32 @hoist(i1 %c, ptr %p) {
entry:
  br i1 %c, label %t, label %f
t:
  %a = load i32, ptr %p, align 4, !invariant.group !0
  br label %join
f:
  %b = load i32, ptr %p, align 4, !invariant.group !1
  br label %join
join:
  %r = phi i32 [ %a, %t ], [ %b, %f ]
  ret i32 %r
}
!0 = !{!"groupA"}
!1 = !{!"groupB"}
```

```
$ opt -S -passes=simplifycfg repro.ll
```

After:
```
entry:
  %a = load i32, ptr %p, align 4, !invariant.group !0
  ret i32 %a
!0 = !{!"groupB"}
```

The kept hoisted load is the `%t`-branch load (`groupA`), but its tag is now `groupB`.

## Why this is a real downstream bug, not "just precision"

A correct conservative merge would either (a) DROP `!invariant.group` from K (since the two paths are not in the same group), or (b) leave K's tag untouched (since K is the surviving instruction). The current code does *neither* — it overwrites K's tag. Downstream consumers of `groupA` semantics (e.g., a `launder.invariant.group` paired with K, or a later GVN-invariant-group-aware load CSE keyed on `groupA`) will silently match against `groupB` or fail to match at all. This is a class of correctness bug recognized by the in-tree FIXME on line 3063.

## Notes
- The `getAllMetadataOtherThanDebugLoc` loop earlier (line 2937) has a `case LLVMContext::MD_invariant_group: break;` at line 2994 (no-op "Preserve in K"). The actual override comes from the epilogue at 3065. The two pieces of code are inconsistent: the switch says "preserve", the epilogue says "blindly take J's".
- Fix would be: if `K->getMetadata(MD_invariant_group)` and JMD differ, set K's to nullptr (drop, conservative).
