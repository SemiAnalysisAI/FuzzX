# w345: Spill/reload missing MOInvariant flag

## Status
SUSPECTED — observable missed optimization, not a miscompile. Spill reloads should be marked invariant because the spill slot is single-defined.

## Source
`llvm/lib/Target/X86/X86InstrBuilder.h:195-210` (`addFrameReference`)
`llvm/lib/Target/X86/X86InstrInfo.cpp:4779-4824` (`X86InstrInfo::storeRegToStackSlot` / `loadRegFromStackSlot`)
`llvm/lib/CodeGen/InlineSpiller.cpp:1189-1203` (`insertReload`)
`llvm/lib/CodeGen/InlineSpiller.cpp:1218-1254` (`insertSpill`)

## Description
When the inline spiller emits a reload via `TII.loadRegFromStackSlot`, X86 builds the reload by calling `addFrameReference`, which constructs:

```cpp
MachineMemOperand *MMO = MF.getMachineMemOperand(
    MachinePointerInfo::getFixedStack(MF, FI, Offset), Flags,
    MFI.getObjectSize(FI), MFI.getObjectAlign(FI));
```

`Flags` is computed solely from `MCID.mayLoad()`/`mayStore()`, so the reload's MMO carries only `MOLoad`. The same slot is also targeted by the matching spill `storeRegToStackSlot`, and outside spill traffic nothing else writes that slot, so every reload from it always returns the value of the most recent dominating spill.

The X86 instruction-folding path that goes through a real memory operand sets `MOInvariant | MODereferenceable` explicitly (cf. `X86InstrInfo.cpp:6055-6058`), but the spill/reload path does not. Reload MMOs therefore lack the `invariant` bit even though the stack slot semantically *is* invariant between the spill and reload.

Downstream this prevents post-RA scheduler / MachineLICM / AA from treating reloads as freely reorderable with respect to unrelated stores or loop-invariant.

## Observed
```
$ cat /tmp/spill_inv3.ll
declare i64 @use(i64, i64, i64, i64, i64, i64, i64, i64)
define i64 @test_spill(ptr %p1, ptr %p2, ptr %p3, ptr %p4, ptr %p5, ptr %p6, ptr %p7, ptr %p8, ptr %p9) {
  %a = load i64, ptr %p1, align 8, !invariant.load !0
  ; ... high reg pressure across 2 calls forces spill of %a
  ret i64 ...
}
!0 = !{}
```
`llc -O2 -mtriple=x86_64-unknown-linux-gnu -stop-after=virtregrewriter`:
```
MOV64mr %stack.3, ..., renamable $rdi :: (store (s64) into %stack.3)
...
$rdi = MOV64rm %stack.3, 1, $noreg, 0, $noreg :: (load (s64) from %stack.3)
```

Note: the IR-level load was `!invariant.load`. After spill the load from `%stack.3` is missing both `invariant` and `dereferenceable` flags. Even if the original load were *not* invariant, the reload still SHOULD be invariant.

## Severity
Missed-opt. No correctness impact alone, but interacts with downstream passes that consult `MMO->isInvariant()`.

## Fix sketch
In `X86InstrInfo::loadRegFromStackSlot`, or in `addFrameReference` when the descriptor only sets `mayLoad()`, OR additionally in the wrappers `InlineSpiller::insertReload` and the reload path of `TargetInstrInfo::foldMemoryOperand` (line 778-785), OR the MMOs of reloads from any fixed stack slot whose object index falls into the spill range: set `MOInvariant | MODereferenceable` for reload MMOs targeting spill slots. The cleanest place is to plumb a `bool IsSpill` parameter through `loadRegFromStackSlot` and add the flags there.
