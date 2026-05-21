# w408: `MemoryLocOrCall::operator==` ignores `!alias.scope`/`!noalias` for `IsCall` queries, breaking the OptimizeUses cache

## Affected analysis

`llvm/lib/Analysis/MemorySSA.cpp:193-206` (`MemoryLocOrCall::operator==`)
and `MemorySSA.cpp:228-241` (DenseMapInfo hash):

```cpp
bool operator==(const MemoryLocOrCall &Other) const {
  if (IsCall != Other.IsCall)
    return false;

  if (!IsCall)
    return Loc == Other.Loc;

  if (Call->getCalledOperand() != Other.Call->getCalledOperand())
    return false;

  return Call->arg_size() == Other.Call->arg_size() &&
         std::equal(Call->arg_begin(), Call->arg_end(),
                    Other.Call->arg_begin());
}
```

For non-call locations the equality check is `Loc == Other.Loc`, and
`MemoryLocation::operator==` compares **all** fields including `AATags`
(TBAA, scope, noalias). For call locations the equality check is just
`(CalledOperand, arg_size, args...)` and **ignores the call's `AAMetadata`
entirely**.

That metadata is materially load-bearing for AA. Two calls to the same
intrinsic with the same operands but different `!alias.scope` /
`!noalias` metadata are *intentionally* distinguishable by AA — that's
the whole point of attaching scope metadata. Two call sites that share an
opcode and arguments but have disjoint `!alias.scope` sets should produce
different `MemoryLocOrCall` keys; otherwise the `OptimizeUses`
`LocStackInfo` cache at `MemorySSA.cpp:1400` will return the wrong
`LastKill`/`LowerBound` for the second call.

## Concrete failure pattern

`OptimizeUses::optimizeUsesInBlock` (line 1399-1497) keys its
`LocStackInfo` map on `MemoryLocOrCall(MU)`. The cache records, per key:

* `LowerBound` — the highest stack index already proven not to clobber
* `LastKill` — the most recent provably-clobbering definition

When two call-MemoryUses with the **same** opcode and args but **different**
scope metadata share a key, the second call inherits the first call's
`LowerBound` / `LastKill`. But the second call has a different scope
visibility — there may be an intervening `MemoryDef` (e.g. a store with a
`!noalias` matching the second call's scope but not the first) that is a
valid clobber for the second call yet was skipped under the first call's
scope domain.

Because the cache says "we already walked past this point", the second
call's defining access is silently set to the prior call's resolved
clobber, which is **higher** in the stack than the true clobber.

## Reduced reproducer (no opt-pipeline miscompile yet, but MSSA dump shows the cache collision)

```llvm
target triple = "x86_64-unknown-linux-gnu"

declare void @do_thing(ptr) memory(argmem: readwrite)

define void @test(ptr %p, ptr %q) {
entry:
  call void @do_thing(ptr %p), !alias.scope !0
  store i32 0, ptr %q, !noalias !1     ; a store noalias against scope2 only
  call void @do_thing(ptr %p), !alias.scope !1
  ret void
}

!d  = !{!"domain"}
!s1 = !{!"scope1", !d}
!s2 = !{!"scope2", !d}
!0 = !{!s1}
!1 = !{!s2}
```

In `MemorySSA::OptimizeUses`, the second `call void @do_thing(ptr %p)` is
keyed by `MemoryLocOrCall` to the *same* key as the first (same operand,
same args; the `!alias.scope` metadata is dropped at line 193-206).
The cached `LowerBound` from the first call's walk is reused; the
intervening store (which carries a `!noalias` tag relevant only to the
second call's scope) may not be re-considered if the cache says it's
already cleared.

## Why this is dormant today

* For non-call accesses, `MemoryLocation::operator==` properly takes
  `AATags` into account, so the cache works correctly for loads/stores.
* For calls, attaching `!alias.scope` to a call is comparatively rare in
  hand-written IR; most compiler-emitted scopes attach to loads/stores.
* MSSA's downstream walker re-validates via `instructionClobbersQuery`
  in many code paths, masking the cache collision.

But when `inline` introduces scope-bearing calls (e.g. cloned
`!alias.scope` from a noalias function argument that survives as an opaque
call), the collision becomes reachable and silently truncates the walk.

## Affected source

* `llvm/lib/Analysis/MemorySSA.cpp:193-206` — call equality drops `AAMetadata`
* `llvm/lib/Analysis/MemorySSA.cpp:228-241` — hash mirrors the same drop

## Fix

In `MemoryLocOrCall::operator==`, additionally compare
`Call->getAAMetadata()` between the two call sites. Mirror the same change
in the hash combiner at line 228.
