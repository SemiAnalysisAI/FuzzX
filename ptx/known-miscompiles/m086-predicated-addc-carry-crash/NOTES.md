# m086-predicated-addc-carry-crash

Found during the same fuzzer session that filed m083, m084, m085, after
moving the bf16/tf32 prologue scratch into non-output registers
(m085 mitigation):

```text
divergences/active-20260519-190809-m085-fixed/div-1779218165-18b10d0fc44520eb
```

ptxas optimiser fails fatally on the predicated extended-precision
carry-chain pattern below. `-O0` accepts the same input cleanly. On
13.2.78 the failure is a graceful internal compiler error (`C7907`); on
13.0.88 the same input segfaults the optimiser.

## Reduced repro

16 lines, no input, no helpers, no const memory, single output store:

```ptx
.version 8.8
.target sm_103
.address_size 64

.visible .entry k()
{
    .reg .pred  %p<1>;
    .reg .b32   %r<8>;
    .reg .b64   %rd<5>;
    shl.b32       %r2, %r5, 24;
    and.b32       %r1, 19, %r6;
    setp.ne.u32   %p0, 26, %r1;
    @!%p0 add.cc.u32 %r7, 16, %r1;
    @!%p0 addc.u32 %r1, %r1, %r2;
    st.global.u32   [%rd4 + 4], %r1;
}
```

The critical sequence is the predicated `add.cc.u32`/`addc.u32` carry
chain under the same guard. Removing either the `add.cc.u32` or the
`addc.u32` makes the crash disappear; removing the `@!%p0` guard makes
it disappear; reading `%rd4`/`%r5`/`%r6` from uninitialised registers
is what shuts the rest of the program off — those values do not
matter for triggering the bug.

## Behaviour

| ptxas | -O0 | -O1 | -O2 | -O3 |
| --- | --- | --- | --- | --- |
| 13.2.78 (`release 13.2, V13.2.78`) | ok | C7907 internal compiler error | C7907 | C7907 |
| 13.0.88 (`release 13.0, V13.0.88`) | ok | — | — | SIGSEGV |

Reproduce:

```bash
PTXAS=/tmp/cuda-13.2.78-py/nvidia/cu13/bin/ptxas
$PTXAS -arch=sm_103 -O3 known-miscompiles/m086-predicated-addc-carry-crash/reduced.ptx -o /tmp/_t.cubin
```

Observed result (13.2.78):

```text
ptxas fatal   : (C7907) Internal compiler error.
ptxas fatal   : Ptx assembly aborted due to errors
```

## Suppressor

The fuzzer-side suppressor used in the saved run is the existing
`DIV_DISABLE_PREDICATED_CARRY=1` flag, which prevents the generator
from emitting a predicated `add.cc.u32` / `addc.u32` pair. That is
heavier than ideal — predicated non-carry adds are unaffected — but
it is the single env flag that already gates the bug family.
