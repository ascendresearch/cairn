#include "reduce_sum.h"

#include <cuda_runtime.h>
#include <stdint.h>

#include <cstring>
#include <iostream>

int main() {
    const float host_input[] = {1.0F, -2.0F, 4.0F};
    float* device_input = nullptr;
    float* device_output = nullptr;
    if (cudaMalloc(&device_input, sizeof(host_input)) != cudaSuccess
        || cudaMalloc(&device_output, sizeof(float)) != cudaSuccess
        || cudaMemcpy(device_input, host_input, sizeof(host_input), cudaMemcpyHostToDevice)
            != cudaSuccess) {
        cudaFree(device_output);
        cudaFree(device_input);
        return 2;
    }

    const int status = cairn_reduce_sum_f32(device_input, device_output, 3);
    float host_output = 0.0F;
    const cudaError_t copy_status = cudaMemcpy(
        &host_output,
        device_output,
        sizeof(host_output),
        cudaMemcpyDeviceToHost);
    cudaFree(device_output);
    cudaFree(device_input);
    if (status != CAIRN_REDUCE_SUM_F32_OK || copy_status != cudaSuccess) {
        return 3;
    }

    uint32_t output_bits = 0;
    static_assert(sizeof(output_bits) == sizeof(host_output));
    std::memcpy(&output_bits, &host_output, sizeof(output_bits));
    std::cout << "{\"element_count\":3,\"output_bits\":\"0x" << std::hex << output_bits
              << "\",\"schema_version\":1}\n";
    return 0;
}
