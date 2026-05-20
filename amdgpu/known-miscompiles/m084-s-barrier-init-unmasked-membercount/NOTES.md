# m084: SDAG lowering of `amdgcn.s.barrier.init` / `s.barrier.signal.var` forwards unmasked `memberCount` to M0

*Discovery method: code inspection.* Found by reading the gfx12+
named-barrier lowering in `SIISelLowering.cpp::LowerINTRINSIC_W_CHAIN`.

## The bug

`amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/SIISelLowering.cpp:12450-12459`:

```cpp
SDValue CntOp = Op->getOperand(3);
SDValue M0Val;
...
// Member count should be put into M0[ShAmt:+6]
// Barrier ID should be put into M0[5:0]
M0Val =
    SDValue(DAG.getMachineNode(AMDGPU::S_AND_B32, DL, MVT::i32, CntOp,
                               DAG.getTargetConstant(0x3F, DL, MVT::i32)),
            0);                                       // <-- masked value, computed
constexpr unsigned ShAmt = 16;
M0Val = DAG.getNode(ISD::SHL, DL, MVT::i32, CntOp,    // <-- but SHL uses raw CntOp
                    DAG.getShiftAmountConstant(ShAmt, MVT::i32, DL));
```

The `S_AND_B32 CntOp, 0x3F` SDValue is built into `M0Val` and
immediately **overwritten** by the next assignment, which shifts the
**unmasked** `CntOp` left by 16.  The masked value is dead.

For `Intrinsic::amdgcn_s_barrier_init` and the `memberCount != 0`
fallthrough from `amdgcn_s_barrier_signal_var`, bits `CntOp[15:6]`
therefore leak into `M0[31:22]`, above the legal member-count field at
`M0[21:16]`.

Per the AMD gfx12 docs the M0 layout for `s_barrier_init` is
`M0[21:16] = memberCount` (6 bits) and `M0[5:0] = barrierID`.  Bits
above 21 must be zero; with `%cnt >= 64` the in-hardware named-barrier
interprets a corrupted member count.

The GISel path at
`AMDGPUInstructionSelector.cpp:7240-7250` performs the same lowering
correctly (masks before shifting).

## Reproducer

`reduced.ll`:

```llvm
target triple = "amdgcn-amd-amdhsa"

declare void @llvm.amdgcn.s.barrier.init(ptr addrspace(3), i32)
@bar = internal addrspace(3) global i32 poison, align 16

define amdgpu_kernel void @fuzz_kernel(i32 %cnt) #0 {
  call void @llvm.amdgcn.s.barrier.init(ptr addrspace(3) @bar, i32 %cnt)
  ret void
}
attributes #0 = { nounwind "target-cpu"="gfx1200" }
```

gfx12+ only intrinsic, so this cannot be run on the gfx950 HIP harness.
The SDAG vs GISel asm divergence is itself the proof:

```bash
$ llc -mtriple=amdgcn-amd-amdhsa -mcpu=gfx1200 -global-isel=false reduced.ll
        s_lshl_b32 m0, s0, 16        ; <-- WRONG (no &0x3F mask)
        s_barrier_init m0

$ llc -mtriple=amdgcn-amd-amdhsa -mcpu=gfx1200 -global-isel=true  reduced.ll
        s_and_b32  s0, s0, 63        ; mask member count to 6 bits
        s_lshl_b32 m0, s0, 16
        s_barrier_init m0
```

SDAG is the default ISel path for AMDGPU, so the buggy lowering is
what `clang -O2 -mcpu=gfx1200` emits by default for any kernel using
`llvm.amdgcn.s.barrier.init` with a dynamic member count.

## How a fix should look

Trivial -- use the masked SDValue for the shift:

```cpp
SDValue MemberCnt = SDValue(
    DAG.getMachineNode(AMDGPU::S_AND_B32, DL, MVT::i32, CntOp,
                       DAG.getTargetConstant(0x3F, DL, MVT::i32)), 0);
constexpr unsigned ShAmt = 16;
M0Val = DAG.getNode(ISD::SHL, DL, MVT::i32, MemberCnt,
                    DAG.getShiftAmountConstant(ShAmt, MVT::i32, DL));
```

Then OR with the masked `BarID` (which the existing code already does
correctly via the `BarID` SDValue computed at lines 12442-12447).

## Toolchain Results

| Toolchain | Result |
| --- | --- |
| LLVM HEAD with the local PR patches (`build/llvm-fuzzer`) | Reproduces (SDAG missing mask). |
| ROCm 7.1.1 (`/opt/rocm-7.1.1/lib/llvm/bin/llc`) | Reproduces. |
| ROCm 7.2.3 source build | Reproduces. |

The FuzzX box has no gfx12 hardware so this is demonstrated as static
asm divergence (same approach as m079).
