# c014 — f16 WMMA load/store fragments use a half-size MemVT (v8f16/v4f16) for the MachineMemOperand, under-reporting the real 32B/16B access

- region: M-getTgtMemIntrinsic
- file: NVPTXISelLowering.cpp 4285-4317, 4445-4465, 4583-4603
- kind: miscompile
- confidence(finder): 0.2

## Mechanism
For the f16 a/b matrix-load fragments the case sets Info.memVT = MVT::v8f16 (line 4310) and ptrVal = arg0, flags = MOLoad. But per IntrinsicsNVVM.td the a:f16 / b:f16 fragment is !listsplat(llvm_v2f16_ty, 8) = 8 x <2 x half> = 16 half values = 32 bytes (confirmed: the PTX retval is .align 4 .b8 retval0[32]). v8f16 is only 8 halves = 16 bytes, i.e. exactly half. Likewise the c:f16 load (line 4458, v4f16 = 8B) and store_d_f16 (line 4596, v4f16 = 8B) under-report the true 16-byte (4 x <2 x half>) fragment. Every other fragment type (f32->v8f32, s32->v8i32, bf16/s8/u8 with correct v2i32/v4i32/v8i32) is reported at full size; only the f16 entries are halved. Because ptrVal is a real pointer, this MMO participates in alias analysis / memory-dependence in the MI schedulers. An undersized MMO tells AA the load only touches the low 16 bytes (resp. 8 bytes) of the source, so a store into the upper half of the same 32-byte (resp. 16-byte) source region can be judged non-aliasing and legally reordered after... wait, before... the wmma.load, changing the value the fragment loads. Concretely: store to base+16..31, then wmma.load.a.f16 from base reading 0..31 -> scheduler may sink the load above the store (or hoist the store below) since the MMO claims the load ends at base+16, yielding stale upper-half lanes. This is a latent under-approximation; it is uniform and long-standing (since the original 2017 wmma commit), so it may be an intentional 'representative' memVT rather than a typo, hence lower confidence and hard to turn into a guaranteed deterministic miscompile without a specific aliasing store surviving to MI scheduling.

## Trigger
sm_70+ with ptx60+ for the f16 wmma intrinsics; needs a same-region store to the upper half of the fragment source adjacent to the wmma.load/store and a scheduler pass that consults MMO sizes for the reorder.

## IR
```
target triple = "nvptx64-nvidia-cuda"
; fragment source is 32 bytes; MMO claims only 16, so a store to bytes 16..31 may be reordered across the load.
declare {<2 x half>,<2 x half>,<2 x half>,<2 x half>,<2 x half>,<2 x half>,<2 x half>,<2 x half>} @llvm.nvvm.wmma.m16n16k16.load.a.f16.row.stride.p0(ptr, i32)
define void @t(ptr %p, i32 %s, ptr %hi) {
  %r = call {<2 x half>,<2 x half>,<2 x half>,<2 x half>,<2 x half>,<2 x half>,<2 x half>,<2 x half>} @llvm.nvvm.wmma.m16n16k16.load.a.f16.row.stride.p0(ptr %p, i32 %s)
  ret void
}
```

## llc cmd
`-mtriple=nvptx64 -mcpu=sm_70 -mattr=+ptx60 -O2`
