// Standalone ptxas -O1/-O2/-O3 miscompile reproducer.
//
// This file embeds a reduced PTX kernel, assembles it with ptxas (-O0, -O1,
// -O2, and -O3), launches one warp through the CUDA Driver API, and compares
// lane 4's output against a scalar PTX trace.
//
// Build, typical x86 CUDA install:
//   g++ -std=c++17 -O2 repro_ptxas_vsub4_branch_o1.cpp \
//     -I/usr/local/cuda/include -L/usr/local/cuda/lib64/stubs -lcuda \
//     -o repro_ptxas_vsub4_branch_o1
//
// Build, CUDA SBSA install like this machine:
//   g++ -std=c++17 -O2 repro_ptxas_vsub4_branch_o1.cpp \
//     -I/usr/local/cuda/targets/sbsa-linux/include \
//     -L/usr/local/cuda/targets/sbsa-linux/lib/stubs -lcuda \
//     -o repro_ptxas_vsub4_branch_o1
//
// Run:
//   ./repro_ptxas_vsub4_branch_o1 [sm_XX]
//
// Optional:
//   PTXAS=/path/to/ptxas ./repro_ptxas_vsub4_branch_o1 sm_103
//
// Correct scalar behavior:
//   Each lane starts with r1 = tid.x and r2 = 4. Lanes where tid.x != 4 branch
//   to `then_path` and store 0. Lane 4 falls through to:
//
//     vsub4.u32.u32.u32 r1, 0, 4, 4
//
//   `vsub4` performs four independent unsigned byte subtractions. The low byte
//   computes 0 - 4 modulo 256 = 0xfc, and the other three byte lanes compute
//   0 - 0 = 0. The correct lane-4 output is therefore 0x000000fc.
//
// Observed bug with CUDA 13.0 ptxas V13.0.88 on sm_103 and with CUDA 13.2
// Update 1 ptxas V13.2.78 on sm_103:
//   -O0 stores 0x000000fc for lane 4.
//   -O1, -O2, and -O3 store 0x8080807c for lane 4.
//
// SASS root-cause summary:
//   At -O0, ptxas lowers vsub4 through a byte-wise subtract sequence with a
//   0x80808080 bias and a final mask/fixup, producing 0x000000fc. At -O2,
//   ptxas folds the divergent branch to a SEL that uses 0x8080807c directly.
//   That is the biased intermediate value before the final packed-byte fixup.

#include <cuda.h>

#include <array>
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

constexpr int N_THREADS = 32;
constexpr int OUTPUT_WORDS = N_THREADS * 4;
constexpr int OUTPUT_BYTES = OUTPUT_WORDS * 4;
constexpr int CHECK_TID = 4;
constexpr uint32_t EXPECTED = 0x000000fcu;

static const char* kPtx = R"PTX(
.version 8.8
.target sm_103
.address_size 64

.visible .entry fuzz_kernel(
    .param .u64 out_ptr
)
{
    .reg .pred  %p<1>;
    .reg .b32   %r<4>;
    .reg .b64   %rd<3>;

    ld.param.u64    %rd0, [out_ptr];
    mov.u32         %r3, %tid.x;
    mov.u32         %r0, 0;
    mov.u32         %r1, %r3;
    mov.u32         %r2, 4;

    setp.ne.u32     %p0, %r2, %r1;
    @%p0 bra        then_path;
    vsub4.u32.u32.u32 %r1, %r0, %r1, %r2;
    bra             done;

then_path:
    mov.u32         %r1, 0;

done:
    cvta.to.global.u64 %rd1, %rd0;
    mul.wide.u32    %rd2, %r3, 16;
    add.s64         %rd1, %rd1, %rd2;
    st.global.u32   [%rd1 + 0], %r1;
    st.global.u32   [%rd1 + 4], 0;
    st.global.u32   [%rd1 + 8], 0;
    st.global.u32   [%rd1 + 12], 0;
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
    TempDir dir("/tmp/ptxas_vsub4_branch_repro.XXXXXX");
    const std::string ptx_path = dir.path + "/in.ptx";
    const std::string cubin_path = dir.path + "/out.cubin";
    write_text(ptx_path, kPtx);

    pid_t pid = fork();
    if (pid < 0) {
        throw std::runtime_error(std::string("fork failed: ") + std::strerror(errno));
    }
    if (pid == 0) {
        std::string arch_arg = "-arch=" + arch;
        execlp(ptxas.c_str(), ptxas.c_str(), arch_arg.c_str(), opt,
               "-o", cubin_path.c_str(), ptx_path.c_str(), static_cast<char*>(nullptr));
        std::perror("execlp ptxas");
        _exit(127);
    }

    int status = 0;
    if (waitpid(pid, &status, 0) < 0) {
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

static uint32_t run_kernel(const std::vector<char>& cubin) {
    CUmodule module = nullptr;
    CUfunction fn = nullptr;
    CUdeviceptr d_out = 0;
    std::array<uint32_t, OUTPUT_WORDS> out{};

    check(cuModuleLoadData(&module, cubin.data()), "cuModuleLoadData");
    check(cuModuleGetFunction(&fn, module, "fuzz_kernel"), "cuModuleGetFunction");
    check(cuMemAlloc(&d_out, OUTPUT_BYTES), "cuMemAlloc output");
    check(cuMemsetD8(d_out, 0xa5, OUTPUT_BYTES), "cuMemsetD8 output");

    void* params[] = { &d_out };
    check(cuLaunchKernel(fn, 1, 1, 1, N_THREADS, 1, 1, 0, nullptr, params, nullptr),
          "cuLaunchKernel");
    check(cuCtxSynchronize(), "cuCtxSynchronize");
    check(cuMemcpyDtoH(out.data(), d_out, OUTPUT_BYTES), "cuMemcpyDtoH output");

    cuMemFree(d_out);
    cuModuleUnload(module);
    return out[CHECK_TID * 4];
}

static int report(const char* label, uint32_t got) {
    bool ok = got == EXPECTED;
    std::printf("%s tid4 out[0]: got 0x%08x expected 0x%08x%s\n",
                label, got, EXPECTED, ok ? "" : "  MISMATCH");
    return ok ? 0 : 1;
}

int main(int argc, char** argv) {
    try {
        const char* env_ptxas = std::getenv("PTXAS");
        std::string ptxas = env_ptxas ? env_ptxas : "/usr/local/cuda/bin/ptxas";
        if (access(ptxas.c_str(), X_OK) != 0) {
            ptxas = "ptxas";
        }
        std::string arch = (argc >= 2) ? argv[1] : "sm_103";

        std::cout << "Using ptxas: " << ptxas << "\n";
        std::cout << "Using arch:  " << arch << "\n";
        std::cout << "Expected:    0x" << std::hex << EXPECTED << std::dec << "\n\n";

        auto cubin_o0 = compile_ptx(ptxas, arch, "-O0");
        auto cubin_o1 = compile_ptx(ptxas, arch, "-O1");
        auto cubin_o2 = compile_ptx(ptxas, arch, "-O2");
        auto cubin_o3 = compile_ptx(ptxas, arch, "-O3");

        check(cuInit(0), "cuInit");
        CUdevice dev = 0;
        CUcontext ctx = nullptr;
        check(cuDeviceGet(&dev, 0), "cuDeviceGet");
        create_context(&ctx, dev);

        uint32_t out_o0 = run_kernel(cubin_o0);
        uint32_t out_o1 = run_kernel(cubin_o1);
        uint32_t out_o2 = run_kernel(cubin_o2);
        uint32_t out_o3 = run_kernel(cubin_o3);

        int bad_o0 = report("-O0", out_o0);
        int bad_o1 = report("-O1", out_o1);
        int bad_o2 = report("-O2", out_o2);
        int bad_o3 = report("-O3", out_o3);

        cuCtxDestroy(ctx);

        if (bad_o0 == 0 && (bad_o1 != 0 || bad_o2 != 0 || bad_o3 != 0)) {
            std::cout << "\nREPRODUCED: -O0 matches the scalar PTX trace, but optimized ptxas is wrong.\n";
            return 1;
        }
        if (bad_o0 != 0) {
            std::cout << "\nUnexpected: -O0 did not match the scalar PTX trace.\n";
            return 2;
        }
        std::cout << "\nNot reproduced: optimized ptxas matched the scalar PTX trace on this setup.\n";
        return 0;
    } catch (const std::exception& e) {
        std::cerr << "error: " << e.what() << "\n";
        return 2;
    }
}
