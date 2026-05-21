# X86FlagsCopyLowering: HoistMBB terminators scanned but TestPos placed at first terminator, missing a clobber by a terminator before it

File: llvm/lib/Target/X86/X86FlagsCopyLowering.cpp:499-526

## Reasoning

Inside the hoisting loop:

```cpp
// We also need the terminators to not sneakily clobber flags.
if (HasEFLAGSClobber(HoistMBB->getFirstTerminator()->getIterator(),
                     HoistMBB->instr_end()))
  break;

// We found a viable location, hoist our test position to it.
TestMBB = HoistMBB;
TestPos = TestMBB->getFirstTerminator()->getIterator();
```

The check rejects hoist if any *terminator* in HoistMBB clobbers EFLAGS. But `TestPos` is then set to the first terminator. The pass later (line 552) calls `collectCondsInRegs(*TestMBB, TestPos)` which scans backwards from `TestPos`, and uses `promoteCondToReg(TestMBB, TestPos, ...)` (line 750) to insert SETCCr *at* TestPos. The SETcc reads EFLAGS as-of TestPos.

Problem: the body of HoistMBB *before* the first terminator is not re-checked when we re-hoist. The walk does check `HasEFLAGSClobberPath(HoistMBB, TestMBB)` (line 513), which scans the *predecessors* of TestMBB from BeginMBB=HoistMBB exclusively. That visited set inserts `BeginMBB` so `HoistMBB` itself is never scanned for body clobbers. Combined with the terminator-only check above, the *non-terminator* body of HoistMBB is entirely unscanned. If HoistMBB has, for example, an `ADD32rr` mid-body that defines EFLAGS, and EFLAGS comes into HoistMBB live (and the original CopyDef-of-EFLAGS was farther up dominating HoistMBB), the SETcc inserted at TestPos will capture the *wrong* EFLAGS (the value after the mid-body ADD), not the value at the original CopyDefI.

The intended invariant — "no EFLAGS clobber between the original def and the captured-into-reg SETcc" — is broken whenever the def is dominated above HoistMBB and HoistMBB has any non-terminator EFLAGS-defining instruction.

## MIR reproducer sketch

```
bb.0:                      ; this block is the dominating EFLAGS-def block
  CMP32rr %0, %1, implicit-def $eflags     ; def we want to capture
  ; falls through to bb.1

bb.1:                      ; HoistMBB candidate, EFLAGS live-in
  liveins: $eflags
  %5:gr32 = ADD32rr %2, %3, implicit-def $eflags   ; clobbers the EFLAGS we want
  JCC_1 %bb.2, 4, implicit $eflags                 ; terminator

bb.2:                      ; the original copy-def lives here
  liveins: $eflags
  %6:gr32 = COPY $eflags
  $eflags = COPY %6
  JCC_1 %bb.3, 5, implicit $eflags
```

The hoist loop walks up from bb.2 to bb.1 (`TestMBB->isLiveIn(EFLAGS)` true), accepts it because no terminator in bb.1 clobbers EFLAGS, but the in-body ADD32rr in bb.1 is missed.

## Expected wrong outcome

After lowering, `SETCCr` is inserted just before the JCC in bb.1, reading EFLAGS that was clobbered by the ADD32rr — capturing the ADD's flag result rather than the CMP's. Subsequent rewritten JCCs in bb.2/bb.3 use the wrong saved condition, producing a wrong branch direction. Symptom under `llc -O2 -verify-machineinstrs`: usually no verifier error (the IR is well-typed), but a runtime mismatch versus an `-O0` reference.
