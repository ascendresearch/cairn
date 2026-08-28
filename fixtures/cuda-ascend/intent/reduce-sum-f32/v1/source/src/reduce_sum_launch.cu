#include "reduce_sum.h"

#include <cuda_runtime.h>
#include <stddef.h>
#include <stdint.h>

extern "C" __global__ void cairn_reduce_sum_f32_kernel(
    const float* input,
    float* output,
    uint32_t element_count);

namespace {

bool ranges_overlap(const float* input, float* output, uint32_t element_count) {
    const uintptr_t input_begin = reinterpret_cast<uintptr_t>(input);
    const uintptr_t output_begin = reinterpret_cast<uintptr_t>(output);
    const uintptr_t input_bytes = static_cast<uintptr_t>(element_count) * sizeof(float);
    const uintptr_t input_end = input_begin + input_bytes;
    const uintptr_t output_end = output_begin + sizeof(float);
    if (input_end < input_begin || output_end < output_begin) {
        return true;
    }
    return input_begin < output_end && output_begin < input_end;
}

}  // namespace

extern "C" int cairn_reduce_sum_f32(
    const float* input,
    float* output,
    uint32_t element_count) {
    if (input == nullptr || output == nullptr || element_count == 0 || element_count > 256
        || ranges_overlap(input, output, element_count)) {
        return CAIRN_REDUCE_SUM_F32_INVALID_ARGUMENT;
    }

    cairn_reduce_sum_f32_kernel<<<1, 256>>>(input, output, element_count);
    if (cudaGetLastError() != cudaSuccess || cudaDeviceSynchronize() != cudaSuccess) {
        return CAIRN_REDUCE_SUM_F32_CUDA_FAILURE;
    }
    return CAIRN_REDUCE_SUM_F32_OK;
}
