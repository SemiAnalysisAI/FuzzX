# LICM promoteLoopAccessesToScalars drops syncscope on atomic load/store

## Severity
Memory-model miscompile. Changing `syncscope("singlethread")` to default System
scope changes the synchronization contract — instructions that were only
required to be ordered with respect to other ops on the same thread (e.g., a
signal handler use) are now globalized, which is the wrong direction (does not
break loads being seen, but it does change the contract; the inverse
miscompile -- losing System scope and getting SingleThread -- is the dangerous
one). Either direction violates LLVM IR semantics: the IR after the pass is
not equivalent to the IR before.

## Source
`llvm/lib/Transforms/Scalar/LICM.cpp`
- `LoopPromoter::insertStoresInLoopExitBlocks()` ~line 1825: emits
  `new StoreInst(...)` and only calls `NewSI->setOrdering(AtomicOrdering::Unordered)`.
  SyncScope is left at the default `System`.
- `promoteLoopAccessesToScalars()` ~line 2200: emits the preheader
  `LoadInst(...)` and only calls `PreheaderLoad->setOrdering(AtomicOrdering::Unordered)`.
  SyncScope is left at the default `System`.

The original loop loads/stores carry an arbitrary SyncScopeID; the pass throws
it away.

## Repro
```ll
; /tmp/w86/syncscope.ll
target datalayout = "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-f80:128-n8:16:32:64-S128"
target triple = "x86_64-unknown-linux-gnu"

@G = global i32 0, align 4

define i32 @test_singlethread(i32 %n) {
entry:
  br label %loop
loop:
  %i = phi i32 [ 0, %entry ], [ %i.next, %loop ]
  %sum = phi i32 [ 0, %entry ], [ %sum.next, %loop ]
  %v = load atomic i32, ptr @G syncscope("singlethread") unordered, align 4
  %sum.next = add i32 %sum, %v
  store atomic i32 %sum.next, ptr @G syncscope("singlethread") unordered, align 4
  %i.next = add i32 %i, 1
  %cond = icmp slt i32 %i.next, %n
  br i1 %cond, label %loop, label %exit
exit:
  ret i32 %sum.next
}
```

## opt diff
```
$ opt -passes='loop-mssa(licm)' -S syncscope.ll
...
entry:
  %G.promoted = load atomic i32, ptr @G unordered, align 4         ; <-- syncscope("singlethread") DROPPED
  br label %loop
...
exit:
  store atomic i32 %sum.next.lcssa2, ptr @G unordered, align 4     ; <-- syncscope("singlethread") DROPPED
  ret i32 %sum.next.lcssa
```

Both newly created atomic ops are emitted at default SyncScope::System rather
than inheriting the original `syncscope("singlethread")`. This is observable
on x86 in that the singlethread-scoped variant is permitted to be reordered
relative to other memory operations in ways that the system-scoped variant is
not — so any subsequent pass that reasons about SyncScope on the promoted op
will reach the wrong conclusion.

## Fix sketch
Capture the SyncScopeID from the first encountered unordered-atomic load/store
in the use-walk in `promoteLoopAccessesToScalars` (alongside `SawUnorderedAtomic`),
plumb it through the `LoopPromoter` constructor (next to `UnorderedAtomic`),
and call `setSyncScopeID(SSID)` on both the preheader `LoadInst` and each
exit-block `StoreInst`. Also verify that all atomic ops in the alias set agree
on SyncScope; if they do not, refuse to promote.
