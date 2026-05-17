// CUDA inline-PTX variant of the m050-reg-shl-mask-fold ptxas reproducer.
//
// Build this same CUDA source twice and compare the printed output from the
// -O0 and -O2 binaries:
//
//   nvcc -std=c++17 -O2 -arch=sm_103 -Xptxas -O0 \
//     repro_nvcc_inline_ptx.cu -o repro_nvcc_inline_ptx_o0
//
//   nvcc -std=c++17 -O2 -arch=sm_103 -Xptxas -O2 \
//     repro_nvcc_inline_ptx.cu -o repro_nvcc_inline_ptx_o2
//
// Verified on 2026-05-17 with CUDA Toolkit 13.2.1 nvcc/ptxas
// (`release 13.2, V13.2.78`, build `cuda_13.2.r13.2/compiler.37668154_0`).

#include <cuda_runtime.h>

#include <cstdint>
#include <cstdio>
#include <cstdlib>

constexpr int kThreads = 32;
constexpr int kInputWords = 32;
constexpr int kOutputWords = 128;
constexpr uint32_t kSentinel = 0xa5a5a5a5u;

static void check(cudaError_t err, const char* what) {
    if (err != cudaSuccess) {
        std::fprintf(stderr, "%s: %s\n", what, cudaGetErrorString(err));
        std::exit(2);
    }
}

__global__ void repro_kernel(const uint32_t* in, uint32_t* out) {
    asm volatile(
        "{\n\t"
        ".reg .b32 r<34>;\n\t"
        ".reg .b64 rd<8>;\n\t"
        "mov.u64 rd0, %0;\n\t"
        "mov.u64 rd1, %1;\n\t"
        "mov.u32 r32, %%tid.x;\n\t"
        "cvta.to.global.u64 rd2, rd0;\n\t"
        "mul.wide.u32 rd3, r32, 4;\n\t"
        "add.s64 rd2, rd2, rd3;\n\t"
        "ld.global.u32 r2, [rd2];\n\t"
        "mov.u32 r7, 32;\n\t"
        "mov.u32 r11, 32;\n\t"
        "mov.u32 r13, r32;\n\t"
        "mov.u32 r14, r2;\n\t"
        "mov.u32 r17, r32;\n\t"
        "mov.u32 r28, 28;\n\t"
        "shl.b32 r6, 28805, 3;\n\t"
        "shl.b32 r25, r13, 0;\n\t"
        "add.u32 r31, 2740016779, 1431655765;\n\t"
        "add.u32 r29, 4294901760, r31;\n\t"
        "and.b32 r33, r14, 31;\n\t"
        "shl.b32 r21, 52957, r33;\n\t"
        "shl.b32 r1, r17, 21;\n\t"
        "shl.b32 r8, r1, 13;\n\t"
        "shl.b32 r0, r6, 28;\n\t"
        "sub.u32 r9, r8, r29;\n\t"
        "and.b32 r25, 65535, r25;\n\t"
        "bfe.s32 r5, r9, 4, 7;\n\t"
        "and.b32 r33, r5, 31;\n\t"
        "shl.b32 r28, r25, r33;\n\t"
        "add.u32 r26, r17, r28;\n\t"
        "add.u32 r9, r21, r26;\n\t"
        "shl.b32 r31, r0, 0;\n\t"
        "shl.b32 r26, r31, 17;\n\t"
        "shl.b32 r19, r9, 0;\n\t"
        "and.b32 r33, r19, 31;\n\t"
        "shl.b32 r13, 28155, r33;\n\t"
        "and.b32 r33, r13, 31;\n\t"
        "shl.b32 r20, r19, r33;\n\t"
        "add.u32 r13, 65535, r20;\n\t"
        "add.u32 r0, r13, r26;\n\t"
        "cvta.to.global.u64 rd4, rd1;\n\t"
        "mul.wide.u32 rd5, r32, 16;\n\t"
        "add.s64 rd4, rd4, rd5;\n\t"
        "st.global.u32 [rd4], r0;\n\t"
        "}\n"
        :
        : "l"(in), "l"(out)
        : "memory");
}

static uint64_t fnv1a(const uint32_t* words, int n) {
    uint64_t h = 1469598103934665603ull;
    for (int i = 0; i < n; ++i) {
        uint32_t v = words[i];
        for (int b = 0; b < 4; ++b) {
            h ^= static_cast<unsigned char>(v >> (8 * b));
            h *= 1099511628211ull;
        }
    }
    return h;
}

int main() {
    uint32_t h_in[kInputWords] = {
        0xee0536edu, 0x8c3cb0a6u, 0x2a742a5fu, 0xc8aba418u,
        0x66e31dd1u, 0x051a978au, 0xa3521143u, 0x41898afcu,
        0xdfc104b5u, 0x7df87e6eu, 0x1c2ff827u, 0xba6771e0u,
        0x589eeb99u, 0xf6d66552u, 0x950ddf0bu, 0x334558c4u,
        0xd17cd27du, 0x6fb44c36u, 0x0debc5efu, 0xac233fa8u,
        0x4a5ab961u, 0xe892331au, 0x86c9acd3u, 0x2501268cu,
        0xc338a045u, 0x617019feu, 0xffa793b7u, 0x9ddf0d70u,
        0x3c168729u, 0xda4e00e2u, 0x78857a9bu, 0x16bcf454u,
    };
    uint32_t h_out[kOutputWords];
    for (int i = 0; i < kOutputWords; ++i) {
        h_out[i] = kSentinel;
    }

    uint32_t* d_in = nullptr;
    uint32_t* d_out = nullptr;
    check(cudaMalloc(&d_in, sizeof(h_in)), "cudaMalloc input");
    check(cudaMalloc(&d_out, sizeof(h_out)), "cudaMalloc output");
    check(cudaMemcpy(d_in, h_in, sizeof(h_in), cudaMemcpyHostToDevice), "cudaMemcpy input");
    check(cudaMemcpy(d_out, h_out, sizeof(h_out), cudaMemcpyHostToDevice), "cudaMemcpy output sentinel");

    repro_kernel<<<1, kThreads>>>(d_in, d_out);
    check(cudaGetLastError(), "repro_kernel launch");
    check(cudaDeviceSynchronize(), "cudaDeviceSynchronize");
    check(cudaMemcpy(h_out, d_out, sizeof(h_out), cudaMemcpyDeviceToHost), "cudaMemcpy output");
    check(cudaFree(d_out), "cudaFree output");
    check(cudaFree(d_in), "cudaFree input");

    std::printf("threads=%d\n", kThreads);
    bool any = false;
    for (int i = 0; i < kOutputWords; ++i) {
        if (h_out[i] != kSentinel) {
            any = true;
            std::printf("out[%d]=0x%08x\n", i, h_out[i]);
        }
    }
    if (!any) {
        std::printf("no output words changed\n");
    }
    std::printf("hash=0x%016llx\n", static_cast<unsigned long long>(fnv1a(h_out, kOutputWords)));
    return 0;
}
