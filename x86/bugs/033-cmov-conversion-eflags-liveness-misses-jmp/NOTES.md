# X86CmovConversion: checkEFLAGSLive looks only at successors, missing live-in from indirect paths after EFLAGS-clobbering JCC inserted

File: llvm/lib/Target/X86/X86CmovConversion.cpp:585-608, 696-699, 711

## Reasoning

`checkEFLAGSLive(LastCMOV)` (line 585) only consults the parent block's successors' live-in sets. After the conversion, the pass inserts a new `JCC_1` into `MBB` (line 711) and `FalseMBB` is created with `addSuccessor(SinkMBB)` (line 714) but only the original `MBB` gets the JCC. `JCC_1` reads EFLAGS but does not define EFLAGS, so any subsequent code in `SinkMBB` that relied on the original EFLAGS *value* (not killed in `LastCMOV`) is still fine — except for one path:

If `LastCMOV` was the *only* user that killed EFLAGS, but a later instruction (already spliced into `SinkMBB`) reads EFLAGS via an implicit operand that the verifier does not check for liveness (e.g., a `COPY $eflags` that becomes the source for `X86FlagsCopyLowering`, or a `SETCCr` from `getCondFromSETCC` perspective that was not on the immediate-use chain), `checkEFLAGSLive` may return false because it walks only `BB->successors()` *of the original MBB*, querying live-in sets that have not yet been updated to reflect that EFLAGS now must live through the inserted FalseMBB path. The path `MBB -> FalseMBB -> SinkMBB` joins at SinkMBB and the live-in on SinkMBB/FalseMBB is added only inside the `if (checkEFLAGSLive(LastCMOV))` guard.

The combinator that exposes this: a CMOV group where the *last* CMOV kills EFLAGS at the bundle level, but a sibling SETcc instruction earlier in the group has consumed it without a kill flag. After conversion the SETcc has been moved (well, before the LastCMOV in original order) and may now reside in SinkMBB if it followed the last CMOV's iterator. Specifically, line 703 splices `[next(LastCMOV), end)` into SinkMBB — so a setcc *after* LastCMOV that reads EFLAGS goes into SinkMBB, but since `LastCMOV->killsRegister(EFLAGS)` returned true (assuming MIR had a kill marker on the cmov), no liveness is added to SinkMBB. The result: SETcc in SinkMBB reads an EFLAGS that is no longer live-in, producing a verifier failure or, worse, reading whatever EFLAGS was clobbered by the inserted `JCC_1` predecessor (JCC doesn't clobber EFLAGS, so often "ok") OR by the next code that runs.

The key bug surface: relying on the kill marker on the *last* CMOV without checking sibling/following readers of EFLAGS that originally were dominated by the same compare.

## MIR reproducer sketch

```
bb.0:
  liveins: $edi, $esi
  %0:gr32 = COPY $edi
  %1:gr32 = COPY $esi
  CMP32rr %0, %1, implicit-def $eflags
  %2:gr32 = CMOV32rr %0, %1, 4, implicit $eflags        ; CMOV_E
  %3:gr32 = CMOV32rr %1, %0, 4, killed implicit $eflags ; CMOV_E, kills EFLAGS
  %4:gr8  = SETCCr 4, implicit $eflags                  ; <-- reads EFLAGS *after* last cmov
  $eax = COPY %2
  $edx = COPY %3
  $cl  = COPY %4
  RET 0, $eax, $edx, $cl
```

(Force x86-cmov-converter-force-all=true to convert.)

## Expected wrong outcome

After conversion, SinkMBB contains the SETCCr but does not have EFLAGS as a live-in (because `checkEFLAGSLive` returned false), failing `llc -verify-machineinstrs` with "Using an undefined physical register" or producing an incorrect SETCC result based on stale flags. Use `llc -O2 -x86-cmov-converter-force-all -verify-machineinstrs` to surface.
