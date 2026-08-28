#include "compact_above.h"

namespace {

__global__ void compact_above_kernel(
    const float* input,
    uint32_t count,
    float threshold,
    float* output,
    uint32_t* output_count) {
  const uint32_t index = blockIdx.x * blockDim.x + threadIdx.x;
  if (index >= count) {
    return;
  }

  const float value = input[index];
  if (value > threshold) {
    const uint32_t output_index = atomicAdd(output_count, 1U);
    output[output_index] = value;
  }
}

}  // namespace

extern "C" cudaError_t launch_compact_above_f32(
    const float* input,
    uint32_t count,
    float threshold,
    float* output,
    uint32_t* output_count,
    cudaStream_t stream) {
  cudaError_t status = cudaMemsetAsync(output_count, 0, sizeof(*output_count), stream);
  if (status != cudaSuccess || count == 0) {
    return status;
  }

  constexpr uint32_t threads_per_block = 256;
  const uint32_t blocks = (count + threads_per_block - 1) / threads_per_block;
  compact_above_kernel<<<blocks, threads_per_block, 0, stream>>>(
      input, count, threshold, output, output_count);
  return cudaGetLastError();
}
