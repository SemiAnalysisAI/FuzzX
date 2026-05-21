# X86FastTileConfig: tile shapes not emitted at PLDTILECFGV when tile def lives in a different BB

**File:** `llvm/lib/Target/X86/X86FastTileConfig.cpp:119-175`

## Reasoning
`X86FastTileConfigImpl::configBasicBlock` walks each MBB in REVERSE, collecting `ShapeInfos` for every `isTileDef` it sees AFTER the latest `PLDTILECFGV`. When it hits a `PLDTILECFGV`, it emits MOV8mr/MOV16mr stores into the tile-config stack slot for everything it has collected, then clears `ShapeInfos`.

The pass operates strictly per-BB (line 191-192 calls `configBasicBlock(MBB)` independently for each block). So if the AMX tile-def producer (e.g. `PTILELOADDV` defining TMM0 with shape (R1,C1)) lives in BB1, the value is live-out across the CFG, and PreTileConfig has placed a `PLDTILECFGV` in BB2 (because a call in/before BB2 clobbered AMX), the shape stores for the cross-BB tile-def are NEVER written into the tile-config slot before the BB2 PLDTILECFGV.

The pre-config zero-init at PreTileConfig.cpp:417-445 leaves the slot zeroed by default, so the post-call `PLDTILECFGV` loads tile shape = (0 rows, 0 colsb) for TMM0 — silently misconfiguring the tile. A subsequent `TILEDPBSSDV`/`TILESTOREDV` using TMM0 either does nothing or behaves incorrectly.

PreTileConfig uses `ManagedRA` (line 274) and assumes shape defs dominate, but Fast register allocator may produce physical TMMs whose effective live range spans a PLDTILECFGV with no in-BB tile-def to repopulate.

## IR/MIR repro sketch
```
; llc -mtriple=x86_64-- -mattr=+amx-tile,+amx-int8 -O0 fast.ll
define void @f(ptr %A, ptr %B, i1 %c) {
entry:
  %t = call x86_amx @llvm.x86.tileloadd64.internal(i16 8, i16 32, ptr %A, i64 32)
  br i1 %c, label %call, label %use
call:
  call void @opaque()              ; clobbers AMX -> PreTileConfig inserts PLDTILECFGV after the call
  br label %use
use:
  call void @llvm.x86.tilestored64.internal(i16 8, i16 32, ptr %B, i64 32, x86_amx %t)
  ret void
}
```
Expected wrong outcome: the PLDTILECFGV reloaded after `@opaque` has tile0.rows=0, tile0.colsb=0; TILESTORED stores no data (or hits a #UD due to misconfigured tile).
