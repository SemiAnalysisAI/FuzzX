cmake_minimum_required(VERSION 3.20)

project(FuzzXLLVMAMDGPUDiffFuzzer LANGUAGES C CXX)

find_package(LLVM REQUIRED CONFIG)
find_package(LLD REQUIRED CONFIG)

message(STATUS "Using LLVM ${LLVM_PACKAGE_VERSION} from ${LLVM_DIR}")
message(STATUS "Using LLD from ${LLD_DIR}")

list(APPEND CMAKE_MODULE_PATH "${LLVM_CMAKE_DIR}")
include(HandleLLVMOptions)

set(ROCM_PATH "/opt/rocm-7.1.1" CACHE PATH "ROCm installation path")

add_executable(llvm_amdgpu_diff_fuzzer llvm_amdgpu_diff_fuzzer.cpp)
target_compile_features(llvm_amdgpu_diff_fuzzer PRIVATE cxx_std_17)
target_include_directories(llvm_amdgpu_diff_fuzzer PRIVATE
    ${LLVM_INCLUDE_DIRS}
    ${LLD_INCLUDE_DIRS}
    "${ROCM_PATH}/include"
)
target_compile_definitions(llvm_amdgpu_diff_fuzzer PRIVATE
    ${LLVM_DEFINITIONS}
    __HIP_PLATFORM_AMD__
)
target_link_directories(llvm_amdgpu_diff_fuzzer PRIVATE "${ROCM_PATH}/lib")

if(CMAKE_CXX_COMPILER_ID MATCHES "Clang")
    target_compile_options(llvm_amdgpu_diff_fuzzer PRIVATE
        -fno-omit-frame-pointer
        -fsanitize=fuzzer,address
    )
    target_link_options(llvm_amdgpu_diff_fuzzer PRIVATE
        -fsanitize=fuzzer,address
    )
endif()

if(TARGET LLVM)
    target_link_libraries(llvm_amdgpu_diff_fuzzer PRIVATE LLVM)
else()
    llvm_map_components_to_libnames(LLVM_LIBS
        AMDGPUAsmParser
        AMDGPUCodeGen
        AMDGPUDesc
        AMDGPUInfo
        Analysis
        CodeGen
        Core
        MC
        Passes
        Support
        Target
        TargetParser
        TransformUtils
    )
    target_link_libraries(llvm_amdgpu_diff_fuzzer PRIVATE ${LLVM_LIBS})
endif()

target_link_libraries(llvm_amdgpu_diff_fuzzer PRIVATE
    lldELF
    lldCommon
    amdhip64
)
