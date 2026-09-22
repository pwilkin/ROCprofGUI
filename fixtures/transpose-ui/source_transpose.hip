// Synthetic fixture. Values are illustrative, not benchmark evidence.
#include <hip/hip_runtime.h>

template <typename T, int TILE>
__global__ void transpose_tiled(const T* input, T* output,
                                const int width) {
    __shared__ T tile[TILE][TILE + 1];
    const int x = blockIdx.x * TILE + threadIdx.x;
    const int y = blockIdx.y * TILE + threadIdx.y;
    const int offset = y * width + x;
    T value = input[offset];
    tile[threadIdx.y][threadIdx.x] = value;
    __syncthreads();
    T transposed = tile[threadIdx.x][threadIdx.y];
    output[x * width + y] = transposed;
}
