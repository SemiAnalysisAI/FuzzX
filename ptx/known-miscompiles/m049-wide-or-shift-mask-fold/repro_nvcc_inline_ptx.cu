// CUDA inline-PTX variant of the m049-wide-or-shift-mask-fold ptxas reproducer.
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
        "ld.global.u32 r10, [rd2];\n\t"
        "add.u32 r12, r32, 16;\n\t"
        "cvt.u64.u32 rd6, 262144;\n\t"
        "cvt.u64.u32 rd7, 267548771;\n\t"
        "or.b64 rd6, rd6, rd7;\n\t"
        "mov.b64 {r1, r33}, rd6;\n\t"
        "sub.u32 r12, 0, r12;\n\t"
        "shl.b32 r22, r1, 19;\n\t"
        "shr.u32 r1, r12, 13;\n\t"
        "add.u32 r5, r22, r1;\n\t"
        "add.u32 r21, r5, r1;\n\t"
        "and.b32 r11, r21, r10;\n\t"
        "sub.u32 r6, r11, 15029;\n\t"
        "sub.u32 r19, r6, 32;\n\t"
        "add.u32 r1, r22, r19;\n\t"
        "cvta.to.global.u64 rd4, rd1;\n\t"
        "mul.wide.u32 rd5, r32, 16;\n\t"
        "add.s64 rd4, rd4, rd5;\n\t"
        "st.global.u32 [rd4 + 4], r1;\n\t"
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
        0xd267d34cu, 0x709f4d05u, 0x0ed6c6beu, 0xad0e4077u,
        0x4b45ba30u, 0xe97d33e9u, 0x87b4ada2u, 0x25ec275bu,
        0xc423a114u, 0x625b1acdu, 0x00929486u, 0x9eca0e3fu,
        0x3d0187f8u, 0xdb3901b1u, 0x79707b6au, 0x17a7f523u,
        0xb5df6edcu, 0x5416e895u, 0xf24e624eu, 0x9085dc07u,
        0x2ebd55c0u, 0xccf4cf79u, 0x6b2c4932u, 0x0963c2ebu,
        0xa79b3ca4u, 0x45d2b65du, 0xe40a3016u, 0x8241a9cfu,
        0x20792388u, 0xbeb09d41u, 0x5ce816fau, 0xfb1f90b3u,
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
