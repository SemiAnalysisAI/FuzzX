// Standalone ptxas -O2 miscompile reproducer.
//
// This file embeds a reduced PTX kernel, assembles it twice with ptxas (-O0
// and -O2), runs both cubins through the CUDA Driver API, and compares output
// word out[0][1] against the scalar PTX trace.
//
// Build, typical x86 CUDA install:
//   g++ -std=c++17 -O2 repro_ptxas_mulhi_control_fold_o2.cpp \
//     -I/usr/local/cuda/include -L/usr/local/cuda/lib64/stubs -lcuda \
//     -o repro_ptxas_mulhi_control_fold_o2
//
// Build, CUDA SBSA install like this machine:
//   g++ -std=c++17 -O2 repro_ptxas_mulhi_control_fold_o2.cpp \
//     -I/usr/local/cuda/targets/sbsa-linux/include \
//     -L/usr/local/cuda/targets/sbsa-linux/lib/stubs -lcuda \
//     -o repro_ptxas_mulhi_control_fold_o2
//
// Run:
//   ./repro_ptxas_mulhi_control_fold_o2 [sm_XX]
//
// Optional:
//   PTXAS=/path/to/ptxas ./repro_ptxas_mulhi_control_fold_o2 sm_103
//
// Correct PTX behavior for input[0] = 0x55ff25dc and in_n = 32:
//   r11 = 60
//   r18 = 60 * 49375 + 60 = 0x002d3480
//   r5  = 8 - r18 = 0xffd2cb88
//   r6  = 32 * r5 + r5 = 0xfa2c3c88
//   p6  = setp.ge.u32 19682, 0x55ff25dc = false
//   r3  = 0 ^ r5 = 0xffd2cb88
//   r10 = r18 & 2 = 0
//   r0  = 0x40000000
//   r8  = brev.b32 0x20000000 = 4
//   r16 = 0x40000000 ^ 33145 = 0x40008179
//   r2  = popc.b32 24696 = 6
//   r4  = mul.hi.s32 6, 0x40008179 = 1
//   p18 = setp.eq.u32 1, 0 = false
//   r13 = mul.hi.s32 489, 0xffd2cb88 = 0xffffffff
//   r14 = mad.lo.u32 4, 0x20000000, 0xffffffff = 0x7fffffff
//   r19 = 1 - 0x7fffffff = 0x80000002
//   r1  = 12 * 12 + 0x80000002 = 0x80000092
//   p21 = setp.le.u32 4, 0x80000002 = true
//   store r1 to out[0][1]
//
// ptxas -O0 stores 0x80000092. With affected ptxas versions, ptxas -O2
// stores 0x80000090. In the optimized SASS, ptxas folds the final arithmetic
// into a uniform-register add of the high word of 6 * 0x40008179 to
// 0x8000008f. The folded constant should be 0x80000091, so the optimized
// result is two too small.
//
// This reproduced on 2026-05-15 with CUDA Toolkit 13.0 ptxas V13.0.88 and
// CUDA Toolkit 13.2 Update 1 ptxas V13.2.78, which was the latest NVIDIA CUDA
// Toolkit listed that day.
//
// The program returns 1 when the ptxas bug is reproduced: -O0 matches the
// scalar PTX trace, but -O2 does not.

#include <cuda.h>

#include <cerrno>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <iostream>
#include <sstream>
#include <stdexcept>
#include <string>
#include <sys/wait.h>
#include <unistd.h>
#include <vector>

constexpr uint32_t INPUT0 = 0x55ff25dcu;
constexpr uint32_t IN_N = 32u;
constexpr uint32_t EXPECTED = 0x80000092u;
constexpr uint32_t WRONG = 0x80000090u;
constexpr size_t N_THREADS = 1;
constexpr size_t INPUT_BYTES = 4;
constexpr size_t OUTPUT_BYTES = 16;
constexpr size_t TARGET_OFFSET = 4;

static const char* kPtx = R"PTX(
.version 8.8
.target sm_103
.address_size 64

.visible .entry fuzz_kernel(
    .param .u64 in_ptr,
    .param .u64 out_ptr,
    .param .u32 in_n
)
{
    .reg .pred  %p<26>;
    .reg .b32   %r<22>;
    .reg .b64   %rd<8>;

    ld.param.u64    %rd0, [in_ptr];
    ld.param.u64    %rd1, [out_ptr];
    ld.param.u32    %r0, [in_n];
    cvta.to.global.u64 %rd2, %rd0;
    ld.global.u32   %r2, [%rd2];
    mov.u32         %r17, 0;
    mov.u32         %r3, 0;
    mov.u32         %r7, 0;
    setp.eq.u32     %p24, 0, 1;

    mul.hi.s32      %r14, %r0, 16;
    mov.u32         %r11, 60;
    mad.lo.u32      %r18, %r11, 49375, %r11;
    shr.u32         %r13, 69160512, 25;
    mad.lo.u32      %r5, 4, %r14, %r0;
    setp.eq.u32     %p1, 0, 0;
    @%p1 bra        structured_if_0_then;
    bra             structured_if_0_else;
structured_if_0_then:
    setp.eq.u32     %p3, 0, 0;
    selp.b32        %r19, 8, %r17, %p3;
    sub.u32         %r5, %r19, %r18;
    mad.lo.u32      %r6, %r0, %r5, %r5;
    shr.u32         %r8, 16, 11;
    bra             structured_if_0_done;
structured_if_0_else:
    bra             structured_if_0_done;
structured_if_0_done:
    setp.ge.u32     %p6, 19682, %r2;
    @%p6 bra        structured_if_1_then;
    bra             structured_if_1_else;
structured_if_1_then:
    mad.lo.u32      %r19, %r19, 4194304, 27678;
    clz.b32         %r8, %r17;
    setp.ge.u32     %p9, %r19, 53078;
    selp.b32        %r17, 4294967295, %r7, %p9;
    dp4a.s32.u32    %r10, %r3, %r7, 62161;
    bra             structured_if_1_done;
structured_if_1_else:
    xor.b32         %r3, %r8, %r5;
    mad.lo.u32      %r2, %r18, %r19, 49358;
    mul.hi.s32      %r1, %r13, 33098;
    clz.b32         %r19, %r8;
    cvt.u32.u16     %r2, %r2;
    and.b32         %r10, %r18, %r13;
    mad.lo.u32      %r1, %r3, %r6, 31152;
    mad.lo.u32      %r8, %r19, 19382, %r5;
    shr.u32         %r15, %r19, 11;
    shr.u32         %r15, %r1, 26;
    mad.lo.u32      %r13, %r6, %r5, 1469367838;
    sub.u32         %r6, 12, %r10;
    setp.ge.u32     %p14, %r15, 8;
    selp.b32        %r0, %r1, 1073741824, %p14;
    shr.u32         %r7, 27123, 21;
    brev.b32        %r8, 536870912;
    xor.b32         %r16, %r0, 33145;
    bfe.u32         %r17, %r11, 25, 1;
    popc.b32        %r2, 24696;
    and.b32         %r1, %r0, %r19;
    mul.hi.s32      %r4, %r2, %r16;
    mad.lo.u32      %r12, %r5, %r10, %r4;
    setp.eq.u32     %p18, %r12, %r7;
    @%p18 bra       structured_if_2_then;
    bra             structured_if_2_else;
structured_if_2_then:
    bra             structured_if_2_done;
structured_if_2_else:
    mul.hi.s32      %r13, 489, %r3;
    mad.lo.u32      %r14, %r8, 536870912, %r13;
    sub.u32         %r19, %r4, %r14;
    mad.lo.u32      %r1, %r6, %r6, %r19;
    and.b32         %r4, 24045, %r0;
    bra             structured_if_2_done;
structured_if_2_done:
    bra             structured_if_1_done;
structured_if_1_done:
    setp.le.u32     %p21, %r8, %r19;
    @%p21 bra       structured_if_3_then;
    bra             structured_if_3_else;
structured_if_3_then:
    bra             structured_if_3_done;
structured_if_3_else:
    selp.b32        %r1, 5032, %r17, %p24;
    bra             structured_if_3_done;
structured_if_3_done:
    bra             exit;

exit:
    cvta.to.global.u64 %rd4, %rd1;
    st.global.u32   [%rd4 + 4], %r1;
    ret;
}
)PTX";

static void check(CUresult r, const char* op) {
    if (r == CUDA_SUCCESS) {
        return;
    }
    const char* msg = nullptr;
    cuGetErrorString(r, &msg);
    std::ostringstream os;
    os << op << " failed: " << (msg ? msg : "unknown CUDA error");
    throw std::runtime_error(os.str());
}

struct TempDir {
    std::string path;

    explicit TempDir(const char* pattern) {
        std::vector<char> buf(pattern, pattern + std::strlen(pattern) + 1);
        char* p = mkdtemp(buf.data());
        if (!p) {
            throw std::runtime_error(std::string("mkdtemp failed: ") + std::strerror(errno));
        }
        path = p;
    }

    ~TempDir() {
        unlink((path + "/in.ptx").c_str());
        unlink((path + "/out.cubin").c_str());
        rmdir(path.c_str());
    }
};

static void write_text(const std::string& path, const char* text) {
    std::ofstream f(path);
    if (!f) {
        throw std::runtime_error("failed to open " + path);
    }
    f << text;
}

static std::vector<char> read_binary(const std::string& path) {
    std::ifstream f(path, std::ios::binary);
    if (!f) {
        throw std::runtime_error("failed to open " + path);
    }
    return std::vector<char>(std::istreambuf_iterator<char>(f),
                             std::istreambuf_iterator<char>());
}

static std::vector<char> compile_ptx(const std::string& ptxas,
                                     const std::string& arch,
                                     const char* opt) {
    TempDir dir("/tmp/ptxas_mulhi_control_repro.XXXXXX");
    const std::string ptx_path = dir.path + "/in.ptx";
    const std::string cubin_path = dir.path + "/out.cubin";
    const std::string arch_flag = "-arch=" + arch;
    write_text(ptx_path, kPtx);

    const pid_t pid = fork();
    if (pid < 0) {
        throw std::runtime_error(std::string("fork failed: ") + std::strerror(errno));
    }
    if (pid == 0) {
        execlp(ptxas.c_str(), ptxas.c_str(), arch_flag.c_str(), opt, "-o",
               cubin_path.c_str(), ptx_path.c_str(), static_cast<char*>(nullptr));
        std::fprintf(stderr, "exec ptxas failed: %s\n", std::strerror(errno));
        _exit(127);
    }

    int status = 0;
    if (waitpid(pid, &status, 0) != pid) {
        throw std::runtime_error(std::string("waitpid failed: ") + std::strerror(errno));
    }
    if (!WIFEXITED(status) || WEXITSTATUS(status) != 0) {
        std::ostringstream os;
        os << "ptxas " << opt << " failed with status " << status;
        throw std::runtime_error(os.str());
    }
    return read_binary(cubin_path);
}

static void create_context(CUcontext* ctx, CUdevice dev) {
#if CUDA_VERSION >= 13000
    check(cuCtxCreate(ctx, nullptr, 0, dev), "cuCtxCreate");
#else
    check(cuCtxCreate(ctx, 0, dev), "cuCtxCreate");
#endif
}

static uint32_t launch(const std::vector<char>& cubin) {
    CUmodule module = nullptr;
    CUfunction func = nullptr;
    CUdeviceptr in = 0;
    CUdeviceptr out = 0;

    check(cuModuleLoadData(&module, cubin.data()), "cuModuleLoadData");
    check(cuModuleGetFunction(&func, module, "fuzz_kernel"), "cuModuleGetFunction");
    check(cuMemAlloc(&in, INPUT_BYTES), "cuMemAlloc(in)");
    check(cuMemAlloc(&out, OUTPUT_BYTES), "cuMemAlloc(out)");
    check(cuMemcpyHtoD(in, &INPUT0, sizeof(INPUT0)), "cuMemcpyHtoD(in)");
    check(cuMemsetD8(out, 0, OUTPUT_BYTES), "cuMemsetD8(out)");

    uint32_t n = IN_N;
    void* params[] = {&in, &out, &n};
    check(cuLaunchKernel(func, 1, 1, 1, N_THREADS, 1, 1, 0, nullptr, params, nullptr),
          "cuLaunchKernel");
    check(cuCtxSynchronize(), "cuCtxSynchronize");

    uint32_t value = 0;
    check(cuMemcpyDtoH(&value, out + TARGET_OFFSET, sizeof(value)), "cuMemcpyDtoH");
    cuMemFree(out);
    cuMemFree(in);
    cuModuleUnload(module);
    return value;
}

int main(int argc, char** argv) {
    try {
        const char* env_ptxas = std::getenv("PTXAS");
        const std::string ptxas = env_ptxas ? env_ptxas : "ptxas";
        const std::string arch = argc > 1 ? argv[1] : "sm_103";

        check(cuInit(0), "cuInit");
        CUdevice dev = 0;
        CUcontext ctx = nullptr;
        check(cuDeviceGet(&dev, 0), "cuDeviceGet");
        create_context(&ctx, dev);

        const uint32_t o0 = launch(compile_ptx(ptxas, arch, "-O0"));
        const uint32_t o2 = launch(compile_ptx(ptxas, arch, "-O2"));

        cuCtxDestroy(ctx);

        std::cout << "ptxas: " << ptxas << "\n";
        std::cout << "arch:  " << arch << "\n";
        std::cout << "input[0]: 0x" << std::hex << INPUT0 << "\n";
        std::cout << "in_n:     0x" << std::hex << IN_N << "\n";
        std::cout << "expected scalar PTX output: 0x" << std::hex << EXPECTED << "\n";
        std::cout << "known wrong optimized output: 0x" << std::hex << WRONG << "\n";
        std::cout << "-O0 output: 0x" << std::hex << o0 << "\n";
        std::cout << "-O2 output: 0x" << std::hex << o2 << "\n";

        if (o0 != EXPECTED) {
            std::cerr << "unexpected: -O0 did not match the scalar PTX trace\n";
            return 2;
        }
        if (o2 != EXPECTED) {
            std::cerr << "bug reproduced: -O2 produced the wrong result\n";
            return 1;
        }
        std::cout << "bug not reproduced\n";
        return 0;
    } catch (const std::exception& e) {
        std::cerr << "error: " << e.what() << "\n";
        return 2;
    }
}
