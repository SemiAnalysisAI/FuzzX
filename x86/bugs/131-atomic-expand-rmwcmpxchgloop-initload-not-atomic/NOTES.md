file: llvm/lib/CodeGen/AtomicExpandPass.cpp:1769-1789
also:  llvm/lib/Target/X86/X86ISelLowering.h:903-905

X86 overrides `shouldIssueAtomicLoadForAtomicEmulationLoop()` to
return `false`. In `insertRMWCmpXchgLoop` the InitLoaded created
before the cmpxchg loop is therefore only `setVolatile(IsVolatile)`,
NEVER `setAtomic(Monotonic, SSID)`.

This makes the InitLoaded a plain (possibly volatile) NON-ATOMIC
load that races with other threads' atomic stores to the same
location. The comment at line 1777-1779 explicitly acknowledges:

  // The initial load must be atomic with the same synchronization scope
  // to avoid a data race with concurrent stores.

Reproducer (target: x86_64-unknown-linux-gnu):

  define i32 @nand_vol_ss(ptr %p, i32 %v) {
    %x = atomicrmw volatile nand ptr %p, i32 %v syncscope("singlethread") seq_cst, align 4
    ret i32 %x
  }

Run: `opt -mtriple=x86_64-unknown-linux-gnu -atomic-expand -S`

Observed (atomic flag absent on init load):

  define i32 @nand_vol_ss(ptr %p, i32 %v) {
    %1 = load volatile i32, ptr %p, align 4
    br label %atomicrmw.start
  atomicrmw.start:
    %loaded = phi i32 [ %1, %0 ], [ %newloaded, %atomicrmw.start ]
    %2 = and i32 %loaded, %v
    %new = xor i32 %2, -1
    %3 = cmpxchg volatile ptr %p, i32 %loaded, i32 %new syncscope("singlethread") seq_cst seq_cst, align 4
    ...
  }

Expected `%1 = load atomic volatile i32, ptr %p syncscope("singlethread") monotonic, align 4`.
The `atomic` keyword and syncscope are missing → C++ memory-model
data race with concurrent atomic stores on the same address; UB by
IR semantics, can lead to torn reads on widened types.

Same defect on the partword path: AtomicExpandPass.cpp:1221-1237
(`expandPartwordCmpXchg`) — the InitLoaded only gets volatile,
never atomic+ssid, for the same reason. (X86's MinCmpXchgSizeInBits
is 8 so this is dormant on x86 today but live for any backend
that uses partword expansion and shouldIssueAtomicLoadForAtomicEmulationLoop()=false.)

Affected ops (any X86 atomicrmw expanded via cmpxchg loop):
nand, max, min, umax, umin, fadd, fsub, fmin, fmax, fmaximum,
fminimum, fmaximumnum, fminimumnum, uincwrap, udecwrap,
usubcond, usubsat. Also any i128 RMW via the CmpXChg expansion
when libcalls are unavailable.

Fix: drop the X86 override (or set it to true), and have the
header comment "TODO: For correctness, an atomic load should be
issued for all targets" in TargetLowering.h:2306 be honored. The
override appears to have been a perf optimization that introduces
a memory-model violation.
