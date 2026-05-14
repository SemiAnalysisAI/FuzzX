# m001-seed-050f notes

## Reduced testcase

`reduced.ptx` is a 24-line standalone PTX kernel.  It launches two threads and
stores one `u32` per thread.  The scalar PTX trace requires both threads to
store `0`.

Thread 0 skips `block_2` on its first pass, but it reaches `block_5` with
`%r0 = 1`, so it must take the backedge once, execute `block_2`, set `%r1 = 0`,
and only then exit.

Thread 1 falls through `block_2` before its first visit to `block_5`, so it
also stores `0`.

## SASS root cause

At `-O0`, ptxas preserves the loop test as a per-thread predicate derived from
the per-thread register `R0`:

```sass
ISETP.EQ.U32.AND P0, PT, R0, RZ, PT ;  // tid == 0
MOV R0, 0x1 ;
MOV R4, 0x1 ;
@P0 BRA L_header ;
L_body:
    MOV R4, RZ ;
L_header:
    ISETP.EQ.U32.AND P0, PT, R0, RZ, PT ;
    @P0 BRA L_done ;
    MOV R0, RZ ;
    BRA L_body ;
L_done:
    STG.E ..., R4 ;
```

This is correct: thread 0 cannot exit before it executes `L_body`, because
`R0` is still `1` on its first visit to `L_header`.

At `-O2`, ptxas lowers the loop latch to uniform predicates and a uniform
branch:

```sass
IMAD.MOV.U32 R5, RZ, RZ, 0x1 ;         // r1 = 1
UPLOP3.LUT UP0, UPT, UPT, UPT, UPT, 0x80, 0x8 ; // one-shot loop latch
ISETP.NE.U32.AND P0, PT, R0, RZ, PT ;  // tid != 0
@!P0 BRA L_header ;                    // tid 0 skips body initially
L_body:
    IMAD.MOV.U32 R5, RZ, RZ, RZ ;      // r1 = 0
L_header:
    UPLOP3.LUT UP1, UPT, UPT, UPT, UP0, 0x80, 0x8 ;
    @UP0 UPLOP3.LUT UP0, UPT, UPT, UPT, UPT, 0x40, 0x4 ;
    BRA.U UP1, L_body ;
    STG.E ..., R5 ;
```

The bug is that `UP0` is warp-uniform state, but the loop header has divergent
entry paths.  The fallthrough path (`tid != 0`) executes `L_body` first and
consumes the one-shot uniform latch by clearing `UP0`.  When the deferred
branch path (`tid == 0`) reaches `L_header`, `UP0` is already false, so the
uniform backedge is not taken for thread 0.  Thread 0 therefore stores the stale
pre-body value `R5 = 1`.

In source terms, ptxas is treating the loop condition as uniform because `%r0`
is initialized to a constant and then set to zero unconditionally.  That loses
the required path-local/per-lane behavior when different lanes enter the loop
header from different predecessors.

This exact `UP0`/`UP1`/`BRA.U` shape is emitted by both:

* CUDA 13.0 ptxas V13.0.88 for `sm_103`
* CUDA 13.2 Update 1 ptxas V13.2.78 for `sm_103`
