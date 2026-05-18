// CUDA inline-PTX variant of the m055-prmt-reg-control-eq-fold ptxas reproducer.
//
// Build this same CUDA source twice and compare the printed output from the -O0 and -O2 binaries:
//
//   nvcc -std=c++17 -O2 -arch=sm_103 -Xptxas -O0 \
//     repro_nvcc_inline_ptx.cu -o repro_nvcc_inline_ptx_o0
//
//   nvcc -std=c++17 -O2 -arch=sm_103 -Xptxas -O2 \
//     repro_nvcc_inline_ptx.cu -o repro_nvcc_inline_ptx_o2
//
// Verified on 2026-05-18 with CUDA Toolkit 13.0 nvcc/ptxas
// (`release 13.0, V13.0.88`, build `cuda_13.0.r13.0/compiler.36424714_0`).

#include <cuda_runtime.h>

#include <cstdint>
#include <cstdio>
#include <cstdlib>

constexpr int kThreads = 1;
constexpr int kOutputWords = 4;
constexpr uint32_t kN = 32u;
constexpr uint32_t kInput0 = 0x0b1fcdb5u;
constexpr uint32_t kLane = 18u;
constexpr uint32_t kSentinel = 0xa5a5a5a5u;

static void check(cudaError_t err, const char* what) {
    if (err != cudaSuccess) {
        std::fprintf(stderr, "%s: %s\n", what, cudaGetErrorString(err));
        std::exit(2);
    }
}

__global__ void repro_kernel(uint32_t* out, uint32_t n, uint32_t input, uint32_t lane) {
    asm volatile(
        "{\n\t"
        ".reg .pred p<41>;\n\t"
        ".reg .b32 r<34>;\n\t"
        ".reg .b64 rd<8>;\n\t"
        "mov.u64 rd1, %0;\n\t"
        "mov.u32 r0, %1;\n\t"
        "mov.u32 r2, %2;\n\t"
        "mov.u32 r1, %3;\n\t"
        "mov.u32 r3, r0;\n\t"
        "mov.u32 r5, r1;\n\t"
        "mov.u32 r6, r2;\n\t"
        "mov.u32 r9, r1;\n\t"
        "mov.u32 r10, r2;\n\t"
        "mov.u32 r12, 12;\n\t"
        "mov.u32 r15, r0;\n\t"
        "mov.u32 r17, r1;\n\t"
        "mov.u32 r18, r2;\n\t"
        "mov.u32 r19, r0;\n\t"
        "mov.u32 r20, 20;\n\t"
        "mov.u32 r21, r1;\n\t"
        "mov.u32 r24, 24;\n\t"
        "mov.u32 r28, 28;\n\t"
        "mov.u32 r30, r2;\n\t"
        "prmt.b32 r5, r2, r6, 0xa589;\n\t"
        "and.b32 r33, r5, 65535;\n\t"
        "prmt.b32 r19, r28, 65535, r33;\n\t"
        "cvt.u64.u32 rd6, r2;\n\t"
        "cvt.u64.u32 rd7, r20;\n\t"
        "setp.eq.u64 p1, rd6, rd7;\n\t"
        "cvt.u64.u32 rd6, 32;\n\t"
        "cvt.u64.u32 rd7, r21;\n\t"
        "selp.b64 rd6, rd6, rd7, p1;\n\t"
        "mov.b64 {r3, r33}, rd6;\n\t"
        "and.b32 r33, 15, 31;\n\t"
        "shl.b32 r8, r6, r33;\n\t"
        "setp.ge.u32 p4, r24, r30;\n\t"
        "@!p4 shl.b32 r18, r0, 31;\n\t"
        "setp.eq.u32 p18, r19, r18;\n\t"
        "@p18 not.b32 r3, r15;\n\t"
        "and.b32 r33, r15, 65535;\n\t"
        "setp.gt.u32 p23, r10, 268435456;\n\t"
        "@p23 prmt.b32 r3, r24, r6, r33;\n\t"
        "setp.gt.u32 p26, r3, r3;\n\t"
        "and.b32 r33, r17, 31;\n\t"
        "@!p26 shl.b32 r30, r5, r33;\n\t"
        "and.b32 r33, r9, 65535;\n\t"
        "prmt.b32 r19, 131072, r12, r33;\n\t"
        "cvta.to.global.u64 rd4, rd1;\n\t"
        "st.global.u32 [rd4 + 12], r3;\n\t"
        "}\n"
        :
        : "l"(out), "r"(n), "r"(input), "r"(lane)
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
    uint32_t h_out[kOutputWords];
    for (int i = 0; i < kOutputWords; ++i) {
        h_out[i] = kSentinel;
    }

    uint32_t* d_out = nullptr;
    check(cudaMalloc(&d_out, sizeof(h_out)), "cudaMalloc output");
    check(cudaMemcpy(d_out, h_out, sizeof(h_out), cudaMemcpyHostToDevice), "cudaMemcpy output sentinel");

    repro_kernel<<<1, kThreads>>>(d_out, kN, kInput0, kLane);
    check(cudaGetLastError(), "repro_kernel launch");
    check(cudaDeviceSynchronize(), "cudaDeviceSynchronize");
    check(cudaMemcpy(h_out, d_out, sizeof(h_out), cudaMemcpyDeviceToHost), "cudaMemcpy output");
    check(cudaFree(d_out), "cudaFree output");

    std::printf("threads=%d n=%u lane=%u input0=0x%08x\n", kThreads, kN, kLane, kInput0);
    for (int i = 0; i < kOutputWords; ++i) {
        std::printf("out[%d]=0x%08x\n", i, h_out[i]);
    }
    std::printf("hash=0x%016llx\n", static_cast<unsigned long long>(fnv1a(h_out, kOutputWords)));
    return 0;
}
