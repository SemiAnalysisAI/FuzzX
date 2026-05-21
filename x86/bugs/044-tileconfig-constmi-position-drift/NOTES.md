# X86TileConfig: `ConstPos` becomes stale after `ConstMI` is overwritten, mis-hoisting subsequent shape stores

**File:** `llvm/lib/Target/X86/X86TileConfig.cpp:111-205`

## Reasoning
At line 112-120 the pass computes `ConstPos` = the index (in `MF.front()`) of the `MOV8mi` that writes the palette byte, and saves a pointer `ConstMI` to it. Inside the per-virtreg loop (line 140), each immediate-shape def emits a new `MOV8mi`/`MOV16mi` right after `ConstMI` and then *re-assigns* `ConstMI = NewMI;` (line 189).

Later iterations of the same outer for-loop still use the original numeric `ConstPos` at line 197-199 to decide whether to relocate a non-immediate `DefMI` from "before the palette" to "after ConstMI":

```cpp
if (&MBB == &MF.front() &&
    (unsigned)std::distance(MBB.instr_begin(), Iter) < ConstPos)
  Iter = ConstMI->getIterator();
```

But `ConstPos` was computed once based on the original palette position. After many `MOV8mi`/`MOV16mi` insertions move `ConstMI` further down the entry BB, `ConstPos` still refers to the now-stale palette index. For a non-immediate `DefMI` whose distance happens to be `>= ConstPos` but `<` the current position of `ConstMI`, the code will NOT relocate the insertion, and we emit the MOV8mr/MOV16mr BEFORE the palette write — but the palette MOV8mi is needed first because Pre-config (PreTileConfig.cpp:445) sets palette=1 here. Actually, the bigger problem is the inverse: the relocation can mis-fire for the very FIRST iteration if `DefMI` is between palette and the (already-moved) `ConstMI`, putting the shape store at the wrong location and ordering writes incorrectly relative to the palette init.

A second related concern: when `DefMI` is in `MF.front()` and IS a move-immediate, the immediate `Imm` for offset `RowOffset`/`ColOffset` is re-emitted at every def (line 184-189) — but the assert at line 169-173 only checks equality if `Imm != INT64_MAX`, which guards against differing immediates from the SAME R, but allows multiple immediates for the same offset, leaving the LAST one as the effective value. If the last in iteration order is not the dynamically-final one, the tile config is wrong.

## IR/MIR repro sketch
```
; llc -mtriple=x86_64-- -mattr=+amx-tile,+amx-int8 -O2 -regalloc=greedy ic.ll
; two tile defs whose row/col immediates come from separate move-imms in entry
```
Expected wrong outcome: the shape stored at offset 48+I / 16+2*I is the immediate from the LATER def in MRI iteration order, not the one that dominates the AMX use; the tile is configured with the wrong rows/cols.
