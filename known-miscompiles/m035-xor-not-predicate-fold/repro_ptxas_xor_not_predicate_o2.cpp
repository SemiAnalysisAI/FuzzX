// Standalone ptxas -O2 miscompile reproducer.
//
// This file embeds a small PTX kernel, assembles it twice with ptxas (-O0 and
// -O2), runs both cubins through the CUDA Driver API, and compares output word
// out[4][2] against the scalar PTX trace. It does not read an input buffer or
// in_n; the dummy in_ptr and in_n parameters are kept to match the fuzzer ABI.
//
// Build, typical x86 CUDA install:
//   g++ -std=c++17 -O2 repro_ptxas_xor_not_predicate_o2.cpp \
//     -I/usr/local/cuda/include -L/usr/local/cuda/lib64/stubs -lcuda \
//     -o repro_ptxas_xor_not_predicate_o2
//
// Build, CUDA SBSA install like this machine:
//   g++ -std=c++17 -O2 repro_ptxas_xor_not_predicate_o2.cpp \
//     -I/usr/local/cuda/targets/sbsa-linux/include \
//     -L/usr/local/cuda/targets/sbsa-linux/lib/stubs -lcuda \
//     -o repro_ptxas_xor_not_predicate_o2
//
// Run:
//   ./repro_ptxas_xor_not_predicate_o2 [sm_XX]
//
// Optional:
//   PTXAS=/path/to/ptxas ./repro_ptxas_xor_not_predicate_o2 sm_103
//
// Correct PTX behavior for lane tid.x == 4:
//   r18 = 0
//   r1  = xor.b32 tid.x, 0xffffffff = 0xfffffffb
//   p1  = setp.ge.u32 r1, 32        = true
//   r18 = selp.b32 0x04000000, 0x0000588a, p1 = 0x04000000
//   r2  = shr.u32 r18, 24           = 4
//   store r2 to out[4][2]
//
// For tid.x != 4, the branch skips the xor/compare/select and stores 0.
//
// ptxas -O0 stores 4 for lane 4. With affected ptxas versions, ptxas -O2
// stores 0, as if the xor-by-0xffffffff had been dropped before the unsigned
// compare and the false arm of the selp had been selected. This reproduced on
// 2026-05-15 with CUDA Toolkit 13.0 ptxas V13.0.88 and CUDA Toolkit 13.2
// Update 1 ptxas V13.2.78, which was the latest NVIDIA CUDA Toolkit listed
// that day.
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

constexpr uint32_t EXPECTED = 4u;
constexpr uint32_t WRONG = 0u;
constexpr size_t N_THREADS = 32;
constexpr size_t OUTPUT_BYTES = N_THREADS * 16;
constexpr size_t TARGET_OFFSET = 4 * 16 + 8;

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
    .reg .pred  %p<2>;
    .reg .b32   %r<21>;
    .reg .b64   %rd<8>;

    ld.param.u64    %rd1, [out_ptr];
    mov.u32         %r20, %tid.x;
    mov.u32         %r18, 0;

    setp.ne.u32     %p0, 4, %r20;
    @%p0 bra        after_special;

    xor.b32         %r1, %r20, 4294967295;
    setp.ge.u32     %p1, %r1, 32;
    selp.b32        %r18, 67108864, 22666, %p1;

after_special:
    shr.u32         %r2, %r18, 24;

    cvta.to.global.u64 %rd4, %rd1;
    mul.wide.u32    %rd5, %r20, 16;
    add.s64         %rd4, %rd4, %rd5;
    st.global.u32   [%rd4 + 8], %r2;
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
    TempDir dir("/tmp/ptxas_xor_not_predicate_repro.XXXXXX");
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
    check(cuMemAlloc(&in, 4), "cuMemAlloc(in)");
    check(cuMemAlloc(&out, OUTPUT_BYTES), "cuMemAlloc(out)");
    check(cuMemsetD8(out, 0, OUTPUT_BYTES), "cuMemsetD8(out)");

    uint32_t n = 32;
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
