// CUDA inline-PTX variant of the m052-bfe-reg-pos-fold ptxas reproducer.
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
constexpr uint32_t kSentinel = 0xa5a5a5a5u;

static void check(cudaError_t err, const char* what) {
    if (err != cudaSuccess) {
        std::fprintf(stderr, "%s: %s\n", what, cudaGetErrorString(err));
        std::exit(2);
    }
}

__global__ void repro_kernel(uint32_t* out, uint32_t n) {
    asm volatile(
        "{\n\t"
        ".reg .b32 r<10>;\n\t"
        ".reg .b64 rd<8>;\n\t"
        "mov.u64 rd1, %0;\n\t"
        "mov.u32 r0, %1;\n\t"
        "mov.u32 r8, %%tid.x;\n\t"
        "add.s32 r7, 131072, 3610147258;\n\t"
        "bfe.s32 r0, r0, r7, 9;\n\t"
        "cvta.to.global.u64 rd4, rd1;\n\t"
        "mul.wide.u32 rd5, r8, 16;\n\t"
        "add.s64 rd4, rd4, rd5;\n\t"
        "st.global.u32 [rd4], r0;\n\t"
        "}\n"
        :
        : "l"(out), "r"(n)
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

    repro_kernel<<<1, kThreads>>>(d_out, kN);
    check(cudaGetLastError(), "repro_kernel launch");
    check(cudaDeviceSynchronize(), "cudaDeviceSynchronize");
    check(cudaMemcpy(h_out, d_out, sizeof(h_out), cudaMemcpyDeviceToHost), "cudaMemcpy output");
    check(cudaFree(d_out), "cudaFree output");

    std::printf("threads=%d n=%u\n", kThreads, kN);
    for (int i = 0; i < kOutputWords; ++i) {
        std::printf("out[%d]=0x%08x\n", i, h_out[i]);
    }
    std::printf("hash=0x%016llx\n", static_cast<unsigned long long>(fnv1a(h_out, kOutputWords)));
    return 0;
}
