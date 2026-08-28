#pragma once

#include <cuda_runtime_api.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

cudaError_t launch_compact_above_f32(
    const float* input,
    uint32_t count,
    float threshold,
    float* output,
    uint32_t* output_count,
    cudaStream_t stream);

#ifdef __cplusplus
}
#endif
