cmake_minimum_required(VERSION 3.24)
project(compact_above_f32 LANGUAGES CXX CUDA)

add_library(compact_above_f32 STATIC src/compact_above.cu)
target_include_directories(compact_above_f32 PUBLIC include)
set_target_properties(compact_above_f32 PROPERTIES CUDA_STANDARD 17)
