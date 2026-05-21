# m097: AMDGPUSwLowerLDS leaves `llvm.memcpy`/`memset`/`memmove` on LDS pointers unrewritten

## Bug found: `MemIntrinsic` (memcpy/memset/memmove) on LDS is not lowered

### File:line
- `amdgpu/third_party/llvm-project/llvm/lib/Target/AMDGPU/AMDGPUSwLowerLDS.cpp:639-663`
  (`getLDSMemoryInstructions` whitelists only Load/Store/AtomicRMW/AtomicCmpXchg/AddrSpaceCast)
- The faulty replacement happens at line 572 in `replaceKernelLDSAccesses`, which does
  a blanket `GV->replaceUsesWithIf(replacement)` that also rewrites the pointer
  argument of any `llvm.memcpy.p3.*` / `llvm.memset.p3.*` calls — but since these
  intrinsics are not added to `LDSInstructions`, the subsequent
  `translateLDSMemoryOperationsToGlobalMemory` (line 681) never replaces the call
  with a global-addrspace equivalent.

### Effect
The replacement at line 568-569 rewrites the LDS GV use to
`getelementptr i8, ptr addrspace(3) @llvm.amdgcn.sw.lds.<kernel>, i32 <offset>`
i.e. a pointer into the per-kernel `SwLDS` LDS cell (an 8-byte malloc-pointer
slot in LDS) plus the metadata offset. Memcpy / memset then writes into
**LDS** at that bogus location — silently corrupting the malloc-pointer cell
and any adjacent LDS, instead of writing into the global memory buffer that
backs the lowered LDS. ASAN cannot see the access either, so OOB through
memcpy/memset bypasses the sanitizer.

### Repro
`/tmp/swlowerlds/repro_memcpy.ll`:
```
@lds.buf = internal addrspace(3) global [16 x i8] poison, align 4

define amdgpu_kernel void @fuzz_kernel(ptr addrspace(1) %src) #0 {
  %p = getelementptr [16 x i8], ptr addrspace(3) @lds.buf, i32 0, i32 0
  call void @llvm.memset.p3.i32(ptr addrspace(3) %p, i8 0, i32 16, i1 false)
  call void @llvm.memcpy.p3.p1.i32(ptr addrspace(3) %p, ptr addrspace(1) %src, i32 16, i1 false)
  ret void
}

attributes #0 = { sanitize_address ... "target-cpu"="gfx950" }
!0 = !{i32 4, !"nosanitize_address", i32 1}
!1 = !{i32 1, !"amdhsa_code_object_version", i32 500}
```
Run:
```
opt -mtriple=amdgcn-amd-amdhsa -mcpu=gfx950 -passes=amdgpu-sw-lower-lds -S repro_memcpy.ll
```
Observed output (excerpt):
```
%20 = getelementptr inbounds i8, ptr addrspace(3) @llvm.amdgcn.sw.lds.fuzz_kernel, i32 %19
%p  = getelementptr [16 x i8], ptr addrspace(3) %20, i32 0, i32 0
call void @llvm.memset.p3.i32(ptr addrspace(3) %p, i8 0, i32 16, i1 false)
call void @llvm.memcpy.p3.p1.i32(ptr addrspace(3) %p, ptr addrspace(1) %src, i32 16, i1 false)
```
The memset / memcpy survive into the LDS addrspace, targeting `SwLDS + offset`
in LDS instead of `mallocPtr + offset` in global. Subsequent passes
(`LowerMemIntrinsics`) expand them into `ds_write*` writes into the
malloc-pointer LDS cell.

### Same bug class also affects:
- `lowerNonKernelLDSAccesses` (line 1109) which uses the same
  `getLDSMemoryInstructions` whitelist — non-kernel callees with memcpy/memset
  on LDS will be miscompiled the same way.
- `MemMoveInst` (not handled either).
- `llvm.memcpy.element.unordered.atomic.p3.*` variants.

## Rule-outs (no bug found in these areas)

- **Alignment of `SwLDS` and `SwDynLDS`**: `populateSwMetadataGlobal:487-489`
  sets both to `MaxAlignment` (max over all per-kernel LDS GV alignments,
  initialised at line 396 with `Align(1)`). Verified with `align 128` GV →
  `SwLDS` ends up `align 128`. ✓
- **Per-kernel layout offsets**: For `{align-32 i8, align-4 [4xi32]}`,
  metadata items `{0,8,32},{32,1,32},{64,16,32}` are correct;
  redzone offsets `(8,24),(33,31),(80,16)` match. ✓
- **Dynamic LDS COV5 path**: Hidden-arg index 15 is loaded (line 869-872),
  metadata fields offset/size/alignedSize are stored runtime-correctly. ✓
- **Indirect callee LDS use**: With `sanitize_address` on the callee and the
  callee accessing `@lds.shared` (no direct kernel access), `lowerNonKernelLDSAccesses`
  emits the base-table + offset-table lookup via `llvm.amdgcn.lds.kernel.id` ✓.
  Tested with `/tmp/swlowerlds/repro_indirect.ll`.
- **AtomicRMW/CmpXchg on LDS**: alignment, ordering, sync-scope, volatile flag
  all forwarded to the new global atomic at lines 713-731. Verified with
  `seq_cst` / `acq_rel acquire`. ✓
- **Alloca migration**: lines 790-796 splice constant-sized allocas into
  `WIdBlock` so they remain static allocas. Verified with addrspace(5) alloca. ✓
- **`ptrtoint` of LDS pointer escape**: works because LDS layout and the
  malloc'd global layout are parallel (LDS GEPs use the same offset that
  global GEPs do). ✓
- **Wrong `Indices[]` use in `populateLDSToReplacementIndicesMap`**:
  always `{0, Idx, 0}` — but `replaceKernelLDSAccesses:562-564` then synthesises
  the constant GEP `{0, Idx, 0}` which targets `LDSItemTy.field[0]`
  (= `StartOffset`). Consistent. ✓

## Notes
- Multiple dynamic-LDS globals: this pass stacks them (each gets a distinct
  region in global memory `mallocPtr + ...`), so two `external addrspace(3)`
  arrays no longer alias as they would in real LDS. May or may not be a bug
  depending on user expectation; appears to be the pass's design choice. Not
  flagged as a bug.

## Suggested fix sketch
In `getLDSMemoryInstructions`, additionally collect `MemIntrinsic` /
`AnyMemIntrinsic` instructions whose dest (and/or src) is `LOCAL_ADDRESS`,
then in `translateLDSMemoryOperationsToGlobalMemory` rebuild the intrinsic
with `getTranslatedGlobalMemoryPtrOfLDS()`-translated dest (and src) pointer
in `GLOBAL_ADDRESS` and add it to `AsanInfo.Instructions` so it gets
sanitizer-instrumented like load/store.
