# w80 investigation notes - DAGCombiner integer combines

Investigated hot patterns in `llvm/lib/CodeGen/SelectionDAG/DAGCombiner.cpp`:
- `visitADD`, `visitSUB`, `visitMUL`, `visitSHL/SRL/SRA`, `visitAND/OR/XOR`, SETCC folds
- `combineShiftOfShiftedLogic`, `foldAndOrOfSETCC`, BSWAP rotate/shift patterns

## Tests run (all produced correct output)
- `(a+b)-b -> a` (all widths incl. i17, i32, i64, vectors)
- `(X^-1)+1 -> -X`
- `(X^C1)+C2`, `(X|C1)^C2`, `(X+C1)^C2`, `(X+C1)|C2`
- `(X<<1)|(X>>31)` rotate patterns at multiple amounts (1, 5, 8, 17, 27)
- `(X>>C1)<<C2`, `(X<<C1)>>C2` mask folds
- `(bswap X)>>24`, `(bswap X)&0xff`, `(bswap X)>>16`
- `manual_fshl` (or-of-shifts to `shldl`)
- `uadd.with.overflow` and `select(carry,...)` chain
- SETCC swap: `not(slt) -> sge`, `(X+1)<u X -> X == UINT_MAX`
- nsw vs no-nsw `(X+5) <s (X+3)`
- `(X & 0xff) + (X & 0x100) -> X & 0x1ff`
- `(or A B) == 0 -> A==0 & B==0` (via test for sete)
- `(X | C) == C -> (X & ~C) == 0`
- Bit-tests, vector eq across `<4 x i32>`
- `(X+C1) ult C2` constant folding
- `addcmp` cross-cancellation `(a+b cmp a+c)`

## Fuzz runs (all clean)
- 50 seeds of `w80_fuzz2.py` (scalar i32/i64/i16, 30 funcs each)
- 100 seeds of `w80_fuzz3.py` (with select/icmp)
- 200 seeds of `w80_fuzz4.py` (3-arg, deeper chains, select+icmp+arith mix)
- 100 seeds of opt-O3 vs no-opt comparison
- All under `-mtriple=x86_64-linux-gnu`, with `-mattr=+avx512f,+bmi2` and default
- Verified via O0 vs O3 runtime differential, gcc-linked driver

## Files
- `/tmp/w80_fuzz{2,3,4}.py` - generators
- `/tmp/w80_runner.c`, `/tmp/wf*_o{0,3}` - runtime binaries

## Conclusion
No reproducible miscompiles found in DAGCombiner integer combines within the time
budget. The fuzzed area is well covered by existing tests. Real bugs here likely
require either:
- Target-specific shifts/rotates with custom legalization paths
- Specific Loop/CGP/IRTranslator interactions feeding unusual DAG shapes
- Vector lowering edges that DAGCombiner mishandles (out of scope for "integer combines")

No w80 candidate worth filing.
