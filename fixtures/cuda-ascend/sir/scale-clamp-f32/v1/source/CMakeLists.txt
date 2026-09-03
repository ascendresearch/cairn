cmake_minimum_required(VERSION 3.24)
project(scale_clamp_f32 LANGUAGES CXX CUDA)

add_library(scale_clamp_f32 STATIC src/scale_clamp.cu)
target_include_directories(scale_clamp_f32 PUBLIC include)
set_target_properties(scale_clamp_f32 PROPERTIES CUDA_STANDARD 17)
