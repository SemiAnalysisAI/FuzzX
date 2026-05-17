// CUDA inline-PTX variant of the m048-intmax-popc-sub-mask-fold ptxas reproducer.
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
constexpr int kOutputWords = 128;
constexpr uint32_t kSentinel = 0xa5a5a5a5u;

static constexpr uint32_t kInput[kThreads] = {
    0xf3cb6a37u, 0x9202e3f0u, 0x303a5da9u, 0xce71d762u,
    0x6ca9511bu, 0x0ae0cad4u, 0xa918448du, 0x474fbe46u,
    0xe58737ffu, 0x83beb1b8u, 0x21f62b71u, 0xc02da52au,
    0x5e651ee3u, 0xfc9c989cu, 0x9ad41255u, 0x390b8c0eu,
    0xd74305c7u, 0x757a7f80u, 0x13b1f939u, 0xb1e972f2u,
    0x5020ecabu, 0xee586664u, 0x8c8fe01du, 0x2ac759d6u,
    0xc8fed38fu, 0x67364d48u, 0x056dc701u, 0xa3a540bau,
    0x41dcba73u, 0xe014342cu, 0x7e4bade5u, 0x1c83279eu
};

static void check(cudaError_t err, const char* what) {
    if (err != cudaSuccess) {
        std::fprintf(stderr, "%s: %s\n", what, cudaGetErrorString(err));
        std::exit(2);
    }
}

__global__ void repro_kernel(const uint32_t* in, uint32_t* out, uint32_t n) {
    asm volatile(
        "{\n\t"
        ".reg .pred p<23>;\n\t"
        ".reg .b32 r<50>;\n\t"
        ".reg .b64 rd<8>;\n\t"
        "mov.u64 rd0, %0;\n\t"
        "mov.u64 rd1, %1;\n\t"
        "mov.u32 r0, %2;\n\t"
        "mov.u32         r48, %%tid.x;\n\t"
        "cvta.to.global.u64 rd2, rd0;\n\t"
        "mul.wide.u32    rd3, r48, 4;\n\t"
        "add.s64         rd2, rd2, rd3;\n\t"
        "ld.global.u32   r2, [rd2];\n\t"
        "mov.u32         r1, r48;\n\t"
        "mov.u32         r3, r0;\n\t"
        "mov.u32         r4, 4;\n\t"
        "mov.u32         r5, r48;\n\t"
        "mov.u32         r6, r2;\n\t"
        "mov.u32         r7, r0;\n\t"
        "mov.u32         r8, 8;\n\t"
        "mov.u32         r9, r48;\n\t"
        "mov.u32         r10, r2;\n\t"
        "mov.u32         r11, r0;\n\t"
        "mov.u32         r12, 12;\n\t"
        "mov.u32         r13, r48;\n\t"
        "mov.u32         r14, r2;\n\t"
        "mov.u32         r15, r0;\n\t"
        "mov.u32         r16, 16;\n\t"
        "mov.u32         r17, r48;\n\t"
        "mov.u32         r18, r2;\n\t"
        "mov.u32         r19, r0;\n\t"
        "mov.u32         r20, 20;\n\t"
        "mov.u32         r21, r48;\n\t"
        "mov.u32         r22, r2;\n\t"
        "mov.u32         r23, r0;\n\t"
        "mov.u32         r24, 24;\n\t"
        "mov.u32         r25, r48;\n\t"
        "mov.u32         r26, r2;\n\t"
        "mov.u32         r27, r0;\n\t"
        "mov.u32         r28, 28;\n\t"
        "mov.u32         r29, r48;\n\t"
        "mov.u32         r30, r2;\n\t"
        "mov.u32         r31, r0;\n\t"
        "mov.u32         r32, 32;\n\t"
        "mov.u32         r33, r48;\n\t"
        "mov.u32         r34, r2;\n\t"
        "mov.u32         r35, r0;\n\t"
        "mov.u32         r36, 36;\n\t"
        "mov.u32         r37, r48;\n\t"
        "mov.u32         r38, r2;\n\t"
        "mov.u32         r39, r0;\n\t"
        "mov.u32         r40, 40;\n\t"
        "mov.u32         r41, r48;\n\t"
        "mov.u32         r42, r2;\n\t"
        "mov.u32         r43, r0;\n\t"
        "mov.u32         r44, 44;\n\t"
        "mov.u32         r45, r48;\n\t"
        "mov.u32         r46, r2;\n\t"
        "mov.u32         r47, r0;\n\t"
        "setp.le.u32   p0, r42, 2488671102;\n\t"
        "@p0 bra   structured_if_0_then;\n\t"
        "bra             structured_if_0_else;\n\t"
        "structured_if_0_then:\n\t"
        "popc.b32      r21, 2052987616;\n\t"
        "popc.b32      r26, r21;\n\t"
        "add.u32       r36, r30, r27;\n\t"
        "setp.lt.u32   p1, r33, 723494775;\n\t"
        "@p1 bra   structured_if_1_then;\n\t"
        "bra             structured_if_1_else;\n\t"
        "structured_if_1_then:\n\t"
        "add.u32       r1, 2147483646, r26;\n\t"
        "sub.u32       r4, 3046743225, r1;\n\t"
        "and.b32       r23, r4, r36;\n\t"
        "structured_if_1_else:\n\t"
        "structured_if_1_done:\n\t"
        "setp.lt.u32   p2, r42, 1019145259;\n\t"
        "structured_if_2_then:\n\t"
        "structured_if_2_else:\n\t"
        "structured_if_2_done:\n\t"
        "setp.lt.u32   p3, r35, r27;\n\t"
        "structured_if_3_then:\n\t"
        "setp.ne.u32   p4, r43, r43;\n\t"
        "structured_if_4_then:\n\t"
        "structured_if_4_else:\n\t"
        "structured_if_4_done:\n\t"
        "setp.ne.u32   p5, 2683960277, 4278255360;\n\t"
        "structured_if_5_then:\n\t"
        "structured_if_5_else:\n\t"
        "structured_if_5_done:\n\t"
        "structured_if_3_else:\n\t"
        "structured_if_3_done:\n\t"
        "setp.ge.u32   p6, r13, r11;\n\t"
        "structured_if_6_then:\n\t"
        "setp.le.u32   p7, r41, r42;\n\t"
        "structured_if_7_then:\n\t"
        "structured_if_7_else:\n\t"
        "structured_if_7_done:\n\t"
        "structured_if_6_else:\n\t"
        "structured_if_6_done:\n\t"
        "setp.le.u32   p8, r14, r25;\n\t"
        "structured_if_8_then:\n\t"
        "structured_if_8_else:\n\t"
        "structured_if_8_done:\n\t"
        "structured_if_0_else:\n\t"
        "setp.lt.u32   p9, 32768, r17;\n\t"
        "structured_if_9_then:\n\t"
        "setp.le.u32   p10, r38, 266618723;\n\t"
        "structured_if_10_then:\n\t"
        "structured_if_10_else:\n\t"
        "structured_if_10_done:\n\t"
        "setp.eq.u32   p11, r19, r47;\n\t"
        "structured_if_11_then:\n\t"
        "structured_if_11_else:\n\t"
        "structured_if_11_done:\n\t"
        "setp.eq.u32   p12, r5, 2;\n\t"
        "structured_if_12_then:\n\t"
        "structured_if_12_else:\n\t"
        "structured_if_12_done:\n\t"
        "setp.lt.u32   p13, r19, 894635186;\n\t"
        "structured_if_13_then:\n\t"
        "structured_if_13_else:\n\t"
        "structured_if_13_done:\n\t"
        "structured_if_9_else:\n\t"
        "structured_if_9_done:\n\t"
        "setp.eq.u32   p14, r46, r23;\n\t"
        "structured_if_14_then:\n\t"
        "setp.ne.u32   p15, r2, 512;\n\t"
        "structured_if_15_then:\n\t"
        "structured_if_15_else:\n\t"
        "structured_if_15_done:\n\t"
        "structured_if_14_else:\n\t"
        "setp.gt.u32   p16, r4, 3712077568;\n\t"
        "structured_if_16_then:\n\t"
        "structured_if_16_else:\n\t"
        "structured_if_16_done:\n\t"
        "setp.lt.u32   p17, r46, r0;\n\t"
        "structured_if_17_then:\n\t"
        "setp.lt.u32   p18, r31, r34;\n\t"
        "structured_if_18_then:\n\t"
        "structured_if_18_else:\n\t"
        "setp.lt.u32   p19, r10, r5;\n\t"
        "structured_if_19_then:\n\t"
        "structured_if_19_else:\n\t"
        "structured_if_19_done:\n\t"
        "structured_if_18_done:\n\t"
        "structured_if_17_else:\n\t"
        "structured_if_17_done:\n\t"
        "structured_if_14_done:\n\t"
        "setp.lt.u32   p20, r28, 3155773991;\n\t"
        "structured_if_20_then:\n\t"
        "structured_if_20_else:\n\t"
        "structured_if_20_done:\n\t"
        "setp.gt.u32   p21, r33, r21;\n\t"
        "structured_if_21_then:\n\t"
        "structured_if_21_else:\n\t"
        "structured_if_21_done:\n\t"
        "structured_if_0_done:\n\t"
        "setp.le.u32   p22, r8, r39;\n\t"
        "structured_if_22_then:\n\t"
        "add.u32       r0, r23, r26;\n\t"
        "structured_if_22_else:\n\t"
        "structured_if_22_done:\n\t"
        "exit:\n\t"
        "cvta.to.global.u64 rd4, rd1;\n\t"
        "mul.wide.u32    rd5, r48, 16;\n\t"
        "add.s64         rd4, rd4, rd5;\n\t"
        "st.global.u32   [rd4 + 0], r0;\n\t"
        "ret;\n\t"
        "}\n"
        :
        : "l"(in), "l"(out), "r"(n)
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

    uint32_t* d_in = nullptr;
    uint32_t* d_out = nullptr;
    check(cudaMalloc(&d_in, sizeof(kInput)), "cudaMalloc input");
    check(cudaMalloc(&d_out, sizeof(h_out)), "cudaMalloc output");
    check(cudaMemcpy(d_in, kInput, sizeof(kInput), cudaMemcpyHostToDevice), "cudaMemcpy input");
    check(cudaMemcpy(d_out, h_out, sizeof(h_out), cudaMemcpyHostToDevice), "cudaMemcpy output sentinel");

    repro_kernel<<<1, kThreads>>>(d_in, d_out, kThreads);
    check(cudaGetLastError(), "repro_kernel launch");
    check(cudaDeviceSynchronize(), "cudaDeviceSynchronize");
    check(cudaMemcpy(h_out, d_out, sizeof(h_out), cudaMemcpyDeviceToHost), "cudaMemcpy output");
    check(cudaFree(d_out), "cudaFree output");
    check(cudaFree(d_in), "cudaFree input");

    std::printf("threads=%d\n", kThreads);
    for (int tid = 0; tid < kThreads; ++tid) {
        std::printf("out[%d]=0x%08x\n", tid * 4, h_out[tid * 4]);
    }
    std::printf("hash=0x%016llx\n", static_cast<unsigned long long>(fnv1a(h_out, kOutputWords)));
    return 0;
}
