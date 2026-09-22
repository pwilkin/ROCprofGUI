#include <hip/hip_runtime.h>
#include <hip/hip_fp16.h>
#include <algorithm>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <iomanip>
#include <iostream>
#include <string>
#include <vector>
#include <type_traits>

static void checked(hipError_t e) { if (e != hipSuccess) { std::cerr << hipGetErrorString(e) << '\n'; std::exit(1); } }

extern "C" __global__ void calibrate_copy(const float4* x, float4* y, size_t n) {
    for (size_t i = blockIdx.x * blockDim.x + threadIdx.x; i < n; i += gridDim.x * blockDim.x) y[i] = x[i];
}

extern "C" __global__ void calibrate_copy_warm(const float4* x, float4* y, size_t n) {
    for (size_t i = blockIdx.x * blockDim.x + threadIdx.x; i < n; i += gridDim.x * blockDim.x) y[i] = x[i];
}

using V4 = float __attribute__((ext_vector_type(4)));
extern "C" __global__ void calibrate_cache(const volatile V4* x, volatile V4* y, size_t n) {
    for (int repeat=0;repeat<64;++repeat)
        for (size_t i=blockIdx.x*blockDim.x+threadIdx.x;i<n;i+=gridDim.x*blockDim.x) y[i]=x[i];
}

template<class T> __global__ void calibrate_vector(T* output, int loops, T multiplier) {
    T a[8];
    #pragma unroll
    for (int j=0;j<8;++j) a[j] = T((threadIdx.x % 8 + j + 1) * 0.01);
    for (int i=0;i<loops;++i) {
        #pragma unroll 16
        for (int k=0;k<16;++k) {
            #pragma unroll
            for (int j=0;j<8;++j) { if constexpr (std::is_same_v<T,float>) a[j] = __builtin_fmaf(a[j], multiplier, T(0.0001)); else a[j] = __builtin_fma(a[j], multiplier, T(0.0001)); }
        }
    }
    #pragma unroll
    for (int j=0;j<8;++j) output[(blockIdx.x*blockDim.x+threadIdx.x)*8+j] = a[j];
}

__global__ void calibrate_packed(__half2* output, int loops, __half2 multiplier) {
    __half2 a[8];
    #pragma unroll
    for (int j=0;j<8;++j) a[j] = __float2half2_rn((threadIdx.x % 8 + j + 1) * 0.01f);
    for (int i=0;i<loops;++i) {
        #pragma unroll 16
        for (int k=0;k<16;++k) {
            #pragma unroll
            for (int j=0;j<8;++j) a[j] = __hfma2(a[j], multiplier, __float2half2_rn(0.0001f));
        }
    }
    #pragma unroll
    for (int j=0;j<8;++j) output[(blockIdx.x*blockDim.x+threadIdx.x)*8+j] = a[j];
}

extern "C" __global__ void calibrate_lds(float* out, int loops) {
    __shared__ volatile float values[256];
    values[threadIdx.x] = float(threadIdx.x); __syncthreads();
    float acc = 0;
    for (int i=0;i<loops;++i) {
        #pragma unroll 16
        for (int j=0;j<16;++j) acc += values[(threadIdx.x+j)&255];
    }
    out[blockIdx.x*256+threadIdx.x] = acc;
}

#if defined(CALIB_WMMA11) || defined(CALIB_WMMA12)
using F8 = float __attribute__((ext_vector_type(8)));
using I8 = int __attribute__((ext_vector_type(8)));
#if defined(CALIB_WMMA12)
using H = _Float16 __attribute__((ext_vector_type(8)));
using B = __bf16 __attribute__((ext_vector_type(8)));
using I = int __attribute__((ext_vector_type(2)));
#else
using H = _Float16 __attribute__((ext_vector_type(16)));
using B = __bf16 __attribute__((ext_vector_type(16)));
using I = int __attribute__((ext_vector_type(4)));
#endif
__global__ void calibrate_matrix(float* out, int loops, int kind) {
#if defined(__HIP_DEVICE_COMPILE__)
    H h = H{} + (_Float16)0.0625; B b = B{} + (__bf16)0.0625; I v = I{} + 0x01010101;
    F8 a = {}, c = F8{} + 0.125f; I8 ia = {}, ic = I8{} + 1;
    for (int i=0;i<loops;++i) {
        #pragma unroll 8
        for (int j=0;j<8;++j) {
#if defined(CALIB_WMMA12)
            if (kind == 0) { a=__builtin_amdgcn_wmma_f32_16x16x16_f16_w32_gfx12(h,h,a); c=__builtin_amdgcn_wmma_f32_16x16x16_f16_w32_gfx12(h,h,c); }
            else if (kind == 1) { a=__builtin_amdgcn_wmma_f32_16x16x16_bf16_w32_gfx12(b,b,a); c=__builtin_amdgcn_wmma_f32_16x16x16_bf16_w32_gfx12(b,b,c); }
            else { ia=__builtin_amdgcn_wmma_i32_16x16x16_iu8_w32_gfx12(true,v,true,v,ia,false); ic=__builtin_amdgcn_wmma_i32_16x16x16_iu8_w32_gfx12(true,v,true,v,ic,false); }
#else
            if (kind == 0) { a=__builtin_amdgcn_wmma_f32_16x16x16_f16_w32(h,h,a); c=__builtin_amdgcn_wmma_f32_16x16x16_f16_w32(h,h,c); }
            else if (kind == 1) { a=__builtin_amdgcn_wmma_f32_16x16x16_bf16_w32(b,b,a); c=__builtin_amdgcn_wmma_f32_16x16x16_bf16_w32(b,b,c); }
            else { ia=__builtin_amdgcn_wmma_i32_16x16x16_iu8_w32(true,v,true,v,ia,false); ic=__builtin_amdgcn_wmma_i32_16x16x16_iu8_w32(true,v,true,v,ic,false); }
#endif
        }
    }
    for (int j=0;j<8;++j) out[(blockIdx.x*256+threadIdx.x)*8+j] = kind==2 ? float(ia[j]+ic[j]) : a[j]+c[j];
#endif
}
#endif

#if defined(CALIB_MFMA)
using H4 = _Float16 __attribute__((ext_vector_type(4)));
using F4 = float __attribute__((ext_vector_type(4)));
__global__ void calibrate_mfma(float* out, int loops) {
#if defined(__HIP_DEVICE_COMPILE__)
    H4 h = H4{} + (_Float16)0.0625; F4 a = {}, b = F4{} + 0.125f;
    for (int i=0;i<loops;++i) {
        #pragma unroll 8
        for (int j=0;j<8;++j) { a=__builtin_amdgcn_mfma_f32_16x16x16f16(h,h,a,0,0,0); b=__builtin_amdgcn_mfma_f32_16x16x16f16(h,h,b,0,0,0); }
    }
    for (int j=0;j<4;++j) out[(blockIdx.x*256+threadIdx.x)*4+j]=a[j]+b[j];
#endif
}
#endif

int main(int argc, char** argv) {
    std::string mode = argc > 1 ? argv[1] : "probe";
    int device = argc > 2 ? std::stoi(argv[2]) : 0;
    checked(hipSetDevice(device)); hipDeviceProp_t p{}; checked(hipGetDeviceProperties(&p,device));
    int runtime=0,driver=0; checked(hipRuntimeGetVersion(&runtime)); checked(hipDriverGetVersion(&driver));
    char pci[64]{}; checked(hipDeviceGetPCIBusId(pci,sizeof(pci),device));
    if (mode=="probe") {
        std::cout << "{\"name\":" << std::quoted(p.name) << ",\"arch\":" << std::quoted(p.gcnArchName) << ",\"pci\":" << std::quoted(pci)
                  << ",\"device\":" << device << ",\"cu\":" << p.multiProcessorCount << ",\"wave\":" << p.warpSize << ",\"memory\":" << p.totalGlobalMem
                  << ",\"l2\":" << p.l2CacheSize << ",\"clockKHz\":" << p.clockRate << ",\"memoryClockKHz\":" << p.memoryClockRate
                  << ",\"runtime\":" << runtime << ",\"driver\":" << driver << "}\n";
        return 0;
    }
    bool profile=argc>3 && std::string(argv[3])=="profile";
    int blocks = p.multiProcessorCount*8, loops=64;
    size_t bytes = std::min(size_t(1024)*1024*1024, std::min(p.totalGlobalMem/16, std::max(size_t(256)*1024*1024,size_t(p.l2CacheSize)*8)));
    if (mode=="cache_copy") bytes=std::max(size_t(32768),size_t(p.l2CacheSize)/4);
    bool copy = mode=="stream_copy" || mode=="cache_copy";
    size_t allocation = copy ? bytes : size_t(blocks)*256*8*sizeof(double);
    void *x=nullptr,*y=nullptr; checked(hipMalloc(&x,allocation));checked(hipMalloc(&y,allocation));
    checked(hipMemset(x,0x3c,allocation));checked(hipMemset(y,0,allocation));checked(hipDeviceSynchronize());
    double operations=0; std::string unit="TFLOP/s", family=mode;
    bool warming=true;
    auto launch=[&]() {
        if(mode=="cache_copy" && !warming) {calibrate_cache<<<blocks,256>>>((const volatile V4*)x,(volatile V4*)y,bytes/16);operations=double(bytes)*2*64;unit="GB/s";}
        else if(copy) { if(warming) calibrate_copy_warm<<<blocks,256>>>((float4*)x,(float4*)y,bytes/16); else calibrate_copy<<<blocks,256>>>((float4*)x,(float4*)y,bytes/16); operations=double(bytes)*2;unit="GB/s"; }
        else if(mode=="fp32") { calibrate_vector<float><<<blocks,256>>>((float*)y,loops,0.9999f);operations=double(blocks)*256*loops*16*8*2; }
        else if(mode=="fp64") { calibrate_vector<double><<<blocks,256>>>((double*)y,loops,0.9999);operations=double(blocks)*256*loops*16*8*2; }
        else if(mode=="fp16_packed") { calibrate_packed<<<blocks,256>>>((__half2*)y,loops,__float2half2_rn(0.999f));operations=double(blocks)*256*loops*16*8*4; }
        else if(mode=="lds") { calibrate_lds<<<blocks,256>>>((float*)y,loops);operations=double(blocks)*256*loops*16*4;unit="GB/s"; }
#if defined(CALIB_WMMA11) || defined(CALIB_WMMA12)
        else if(mode=="wmma_fp16" || mode=="wmma_bf16" || mode=="wmma_int8") { calibrate_matrix<<<blocks,256>>>((float*)y,loops,mode=="wmma_fp16"?0:mode=="wmma_bf16"?1:2);operations=double(blocks)*256/32*loops*8*2*8192;if(mode=="wmma_int8")unit="TOP/s"; }
#endif
#if defined(CALIB_MFMA)
        else if(mode=="mfma_fp16") { calibrate_mfma<<<blocks,256>>>((float*)y,loops);operations=double(blocks)*256/64*loops*8*2*8192; }
#endif
        else {std::cerr << "Unsupported calibration family: " << mode << '\n';std::exit(2);}
        checked(hipGetLastError());
    };
    for(int i=0;i<3;++i) {launch();checked(hipDeviceSynchronize());}
    warming=false;
    hipEvent_t a,b;checked(hipEventCreate(&a));checked(hipEventCreate(&b));
    std::vector<double> times;
    for(int i=0;i<(profile?1:27) && times.size()<size_t(profile?1:9);++i) { checked(hipEventRecord(a));launch();checked(hipEventRecord(b));checked(hipEventSynchronize(b));float ms=0;checked(hipEventElapsedTime(&ms,a,b));if(ms>0 && std::isfinite(ms))times.push_back(ms*1000.); }
    if(times.size()!=size_t(profile?1:9)) {std::cerr<<"Insufficient positive GPU event timings\n";return 3;}
    std::vector<unsigned char> first(64);checked(hipMemcpy(first.data(),y,first.size(),hipMemcpyDeviceToHost));
    if (std::all_of(first.begin(),first.end(),[](unsigned char c){return c==0;})) {std::cerr<<"Calibration output was not written\n";return 4;}
    if(copy) {std::vector<unsigned char> expected(64);checked(hipMemcpy(expected.data(),x,expected.size(),hipMemcpyDeviceToHost));if(first!=expected)return 5;}
    bool valid=true;float first_value;std::memcpy(&first_value,first.data(),sizeof(first_value));
    if(mode=="fp32") {
        float result[16];std::memcpy(result,first.data(),sizeof(result));
        for(int j=0;j<8;++j) {float expected=float((j+1)*0.01);for(int i=0;i<loops*16;++i)expected=std::fma(expected,0.9999f,0.0001f);valid &= std::isfinite(result[j]) && std::abs(result[j]-expected)<1e-5f;}
    } else if(mode=="fp64") {
        double result[8];std::memcpy(result,first.data(),sizeof(result));
        for(int j=0;j<8;++j) {double expected=(j+1)*0.01;for(int i=0;i<loops*16;++i)expected=std::fma(expected,0.9999,0.0001);valid &= std::isfinite(result[j]) && std::abs(result[j]-expected)<1e-10;}
    } else if(mode=="fp16_packed") {
        __half result[32];std::memcpy(result,first.data(),sizeof(result));
        for(int j=0;j<8;++j) {__half expected=__float2half_rn(float((j+1)*0.01));for(int i=0;i<loops*16;++i)expected=__float2half_rn(std::fma(__half2float(expected),__half2float(__float2half_rn(0.999f)),__half2float(__float2half_rn(0.0001f))));valid &= __half2float(result[2*j])==__half2float(expected) && __half2float(result[2*j+1])==__half2float(expected);}
    } else if(mode=="lds") valid = first_value==float(loops*120);
    else if(mode=="wmma_int8") valid = first_value==float(loops*8*2*16+1);
    else if(mode.find("wmma_")==0 || mode=="mfma_fp16") valid = first_value==float(loops*8*2)*0.0625f+0.125f;
    if(!valid) {std::cerr<<"Calibration arithmetic check failed: "<<mode<<'\n';return 6;}
    std::cout << "{\"outputCheck\":\"passed\",\"family\":" << std::quoted(family) << ",\"unit\":" << std::quoted(unit) << ",\"operations\":" << std::setprecision(17) << operations << ",\"workingSetBytes\":" << (copy?2*bytes:allocation) << ",\"samplesUs\":[";
    for(size_t i=0;i<times.size();++i)std::cout<<(i?",":"")<<times[i];
    std::cout << "]}\n";
    checked(hipEventDestroy(a));checked(hipEventDestroy(b));checked(hipFree(x));checked(hipFree(y));
}
