cmake_minimum_required(VERSION 3.24)
project(cairn_reduce_sum_f32_fixture LANGUAGES CXX CUDA)

add_library(cairn_reduce_sum_f32 STATIC
    src/reduce_sum_kernel.cu
    src/reduce_sum_launch.cu
)
target_include_directories(cairn_reduce_sum_f32 PUBLIC include)
target_compile_features(cairn_reduce_sum_f32 PUBLIC cxx_std_17 cuda_std_17)
set_target_properties(cairn_reduce_sum_f32 PROPERTIES
    CUDA_SEPARABLE_COMPILATION ON
    POSITION_INDEPENDENT_CODE ON
)

add_executable(cairn_reduce_sum_f32_reference tests/reference_main.cpp)
target_link_libraries(cairn_reduce_sum_f32_reference PRIVATE cairn_reduce_sum_f32)
