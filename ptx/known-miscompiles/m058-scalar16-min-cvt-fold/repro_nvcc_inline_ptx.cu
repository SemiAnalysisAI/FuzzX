// CUDA inline-PTX variant of the m058-scalar16-min-cvt-fold ptxas reproducer.
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

constexpr int kThreads = 4;
constexpr int kOutputWords = kThreads * 4;
constexpr uint32_t kN = 32u;
constexpr uint32_t kSentinel = 0xa5a5a5a5u;

static void check(cudaError_t err, const char* what) {
    if (err != cudaSuccess) {
        std::fprintf(stderr, "%s: %s\n", what, cudaGetErrorString(err));
        std::exit(2);
    }
}

__global__ void repro_kernel(uint32_t* out, const uint32_t* in, uint32_t n) {
    asm volatile(
        "{\n\t"
        ".reg .pred p<4>;\n\t"
        ".reg .b16 h<4>;\n\t"
        ".reg .b32 r<10>;\n\t"
        ".reg .b64 rd<8>;\n\t"
        "mov.u64 rd0, %0;\n\t"
        "mov.u64 rd1, %1;\n\t"
        "mov.u32 r0, %2;\n\t"
        "mov.u32 r8, %%tid.x;\n\t"
        "cvta.to.global.u64 rd2, rd1;\n\t"
        "mul.wide.u32 rd3, r8, 4;\n\t"
        "add.s64 rd2, rd2, rd3;\n\t"
        "ld.global.u32 r2, [rd2];\n\t"
        "mov.u32 r4, 4;\n\t"
        "cvt.s16.s32 h0, r4;\n\t"
        "cvt.s16.s32 h1, r0;\n\t"
        "min.s16 h2, h0, h1;\n\t"
        "cvt.s32.s16 r1, h2;\n\t"
        "setp.eq.u32 p0, r4, r1;\n\t"
        "@!p0 mad.lo.s32 r2, r8, 25, r8;\n\t"
        "cvta.to.global.u64 rd4, rd0;\n\t"
        "mul.wide.u32 rd5, r8, 16;\n\t"
        "add.s64 rd4, rd4, rd5;\n\t"
        "st.global.u32 [rd4 + 0], r2;\n\t"
        "cvta.to.global.u64 rd2, rd1;\n\t"
        "add.s64 rd2, rd2, rd3;\n\t"
        "ld.global.u32 r2, [rd2];\n\t"
        "cvt.u16.u32 h0, r4;\n\t"
        "cvt.u16.u32 h1, r0;\n\t"
        "min.u16 h2, h0, h1;\n\t"
        "cvt.u32.u16 r1, h2;\n\t"
        "setp.eq.u32 p1, r4, r1;\n\t"
        "@!p1 mad.lo.s32 r2, r8, 25, r8;\n\t"
        "st.global.u32 [rd4 + 4], r2;\n\t"
        "}\n"
        :
        : "l"(out), "l"(in), "r"(n)
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
    uint32_t h_in[kThreads] = {
        0x857a7008u,
        0x23b1e9c1u,
        0xc1e9637au,
        0x6020dd33u,
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

    repro_kernel<<<1, kThreads>>>(d_out, d_in, kN);
    check(cudaGetLastError(), "repro_kernel launch");
    check(cudaDeviceSynchronize(), "cudaDeviceSynchronize");
    check(cudaMemcpy(h_out, d_out, sizeof(h_out), cudaMemcpyDeviceToHost), "cudaMemcpy output");
    check(cudaFree(d_out), "cudaFree output");
    check(cudaFree(d_in), "cudaFree input");

    std::printf("threads=%d n=%u\n", kThreads, kN);
    for (int t = 0; t < kThreads; ++t) {
        std::printf("tid=%d signed=0x%08x unsigned=0x%08x\n", t, h_out[t * 4], h_out[t * 4 + 1]);
    }
    std::printf("hash=0x%016llx\n", static_cast<unsigned long long>(fnv1a(h_out, kOutputWords)));
    return 0;
}
