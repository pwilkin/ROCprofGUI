// VK_LAYER_ROCPROF_capture: records one compute dispatch so rocprof studio can
// replay it in isolation, the Vulkan counterpart of Kerncap for HIP kernels.
//
// Selection (environment):
//   ROCPROF_VKCAP_OUTPUT      directory that receives the capture (required)
//   ROCPROF_VKCAP_NAME        pipeline debug name, or the innermost debug label at
//                             the dispatch with any trailing " (x,y,z)" removed
//   ROCPROF_VKCAP_GROUPS      optional "x,y,z" workgroup counts the dispatch must have
//   ROCPROF_VKCAP_OCCURRENCE  zero-based index among matching dispatches (default 0)
//
// At the selected vkCmdDispatch the layer records copies of every bound buffer range
// immediately before and after the dispatch, so the capture holds both the inputs
// and the outputs the application itself produced. The files are written after the
// submit that contains the dispatch has completed.
#include <vulkan/vk_layer.h>
#include <vulkan/vulkan.h>

#include <algorithm>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <map>
#include <mutex>
#include <set>
#include <string>
#include <unordered_map>
#include <vector>

#define EXPORT extern "C" __attribute__((visibility("default")))

namespace {

template <typename T> void *key(T handle) { return *reinterpret_cast<void **>(handle); }

struct Instance {
    PFN_vkGetInstanceProcAddr gipa = nullptr;
    PFN_vkDestroyInstance destroy = nullptr;
    PFN_vkGetPhysicalDeviceMemoryProperties memory_properties = nullptr;
    PFN_vkGetPhysicalDeviceProperties properties = nullptr;
    uint32_t api_version = VK_API_VERSION_1_0;
};

struct Device {
    VkDevice device = VK_NULL_HANDLE;
    PFN_vkGetDeviceProcAddr gdpa = nullptr;
    VkPhysicalDeviceMemoryProperties memory{};
    VkPhysicalDeviceProperties properties{};
    uint32_t api_version = VK_API_VERSION_1_0;
    std::vector<std::string> extensions;
    std::vector<std::pair<uint32_t, std::vector<uint8_t>>> features;  // sType, body
    std::vector<uint8_t> features10;
    std::vector<uint32_t> unknown_features;
#define FN(name) PFN_vk##name name = nullptr
    FN(DestroyDevice); FN(CreateBuffer); FN(DestroyBuffer); FN(GetBufferMemoryRequirements);
    FN(AllocateMemory); FN(FreeMemory); FN(BindBufferMemory); FN(MapMemory); FN(UnmapMemory);
    FN(CreateShaderModule); FN(CreateComputePipelines); FN(CreatePipelineLayout);
    FN(CreateDescriptorSetLayout); FN(UpdateDescriptorSets); FN(BeginCommandBuffer);
    FN(CmdBindPipeline); FN(CmdBindDescriptorSets); FN(CmdPushConstants); FN(CmdDispatch);
    FN(CmdPipelineBarrier); FN(CmdCopyBuffer); FN(QueueSubmit); FN(QueueSubmit2);
    FN(QueueWaitIdle); FN(SetDebugUtilsObjectNameEXT); FN(CmdBeginDebugUtilsLabelEXT);
    FN(CmdEndDebugUtilsLabelEXT); FN(AllocateDescriptorSets);
#undef FN
};

struct SetLayoutBinding { uint32_t binding, type, count; };
struct PipelineLayout { std::vector<VkDescriptorSetLayout> sets; std::vector<VkPushConstantRange> push; };
struct Pipeline {
    std::vector<uint32_t> spirv;
    std::string entry;
    uint32_t stage_flags = 0, required_subgroup_size = 0, create_flags = 0;
    std::vector<VkSpecializationMapEntry> spec;
    std::vector<uint8_t> spec_data;
    VkPipelineLayout layout = VK_NULL_HANDLE;
    Device *device = nullptr;
};
struct BoundBuffer { uint32_t type; VkBuffer buffer; VkDeviceSize offset, range; };
struct Group { VkBuffer buffer; VkDeviceSize lo, hi; VkBuffer pre = VK_NULL_HANDLE, post = VK_NULL_HANDLE; VkDeviceMemory pre_mem = VK_NULL_HANDLE, post_mem = VK_NULL_HANDLE; };
struct Binding { uint32_t set, binding, element, type; size_t group; VkDeviceSize offset, range; };
struct Pending {
    Device *device;
    Pipeline pipeline;
    std::string name, label;
    std::vector<std::string> labels;
    uint32_t groups[3];
    std::vector<uint8_t> push;
    std::vector<Group> buffers;
    std::vector<Binding> bindings;
    std::vector<std::pair<uint32_t, std::vector<SetLayoutBinding>>> set_layouts;
};
struct CommandState {
    Device *device = nullptr;
    VkPipeline pipeline = VK_NULL_HANDLE;
    std::map<uint32_t, std::pair<VkDescriptorSet, std::vector<uint32_t>>> sets;
    uint8_t push[256]{};
    std::vector<std::string> labels;
    Pending *pending = nullptr;
};

std::mutex lock;
std::unordered_map<void *, Instance *> instances;
std::unordered_map<void *, Device *> devices;
std::unordered_map<VkBuffer, VkDeviceSize> buffer_sizes;
std::unordered_map<VkShaderModule, std::vector<uint32_t>> modules;
std::unordered_map<VkDescriptorSetLayout, std::vector<SetLayoutBinding>> set_layouts;
std::unordered_map<VkPipelineLayout, PipelineLayout> pipeline_layouts;
std::unordered_map<VkPipeline, Pipeline> pipelines;
std::unordered_map<uint64_t, std::string> names;
std::unordered_map<VkDescriptorSet, std::map<uint64_t, BoundBuffer>> descriptor_sets;
std::unordered_map<VkDescriptorSet, VkDescriptorSetLayout> set_layout_of;
std::unordered_map<VkCommandBuffer, CommandState> commands;
uint64_t matched = 0;
bool armed = false, finished = false;

const char *env(const char *name) { const char *v = getenv(name); return v && *v ? v : nullptr; }
bool active() { return env("ROCPROF_VKCAP_OUTPUT") != nullptr; }

void fail(const std::string &message) {
    std::string path = std::string(env("ROCPROF_VKCAP_OUTPUT")) + "/capture_error";
    if (FILE *f = fopen(path.c_str(), "w")) { fputs(message.c_str(), f); fclose(f); }
    fprintf(stderr, "rocprof capture layer: %s\n", message.c_str());
    finished = true;
}

// Feature structures the replay re-enables verbatim. Anything else in the chain is
// listed as unknown so the replay can say its device may differ.
size_t feature_size(VkStructureType t) {
    switch (t) {
#define F(s, T) case s: return sizeof(T)
        F(VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_VULKAN_1_1_FEATURES, VkPhysicalDeviceVulkan11Features);
        F(VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_VULKAN_1_2_FEATURES, VkPhysicalDeviceVulkan12Features);
        F(VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_VULKAN_1_3_FEATURES, VkPhysicalDeviceVulkan13Features);
        F(VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_VULKAN_1_4_FEATURES, VkPhysicalDeviceVulkan14Features);
        F(VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_16BIT_STORAGE_FEATURES, VkPhysicalDevice16BitStorageFeatures);
        F(VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_8BIT_STORAGE_FEATURES, VkPhysicalDevice8BitStorageFeatures);
        F(VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_SHADER_FLOAT16_INT8_FEATURES, VkPhysicalDeviceShaderFloat16Int8Features);
        F(VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_SUBGROUP_SIZE_CONTROL_FEATURES, VkPhysicalDeviceSubgroupSizeControlFeatures);
        F(VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_COOPERATIVE_MATRIX_FEATURES_KHR, VkPhysicalDeviceCooperativeMatrixFeaturesKHR);
        F(VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_SHADER_INTEGER_DOT_PRODUCT_FEATURES, VkPhysicalDeviceShaderIntegerDotProductFeatures);
        F(VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_MAINTENANCE_4_FEATURES, VkPhysicalDeviceMaintenance4Features);
        F(VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_BUFFER_DEVICE_ADDRESS_FEATURES, VkPhysicalDeviceBufferDeviceAddressFeatures);
        F(VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_SYNCHRONIZATION_2_FEATURES, VkPhysicalDeviceSynchronization2Features);
        F(VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_SHADER_BFLOAT16_FEATURES_KHR, VkPhysicalDeviceShaderBfloat16FeaturesKHR);
        F(VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_PIPELINE_ROBUSTNESS_FEATURES, VkPhysicalDevicePipelineRobustnessFeatures);
        F(VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_VULKAN_MEMORY_MODEL_FEATURES, VkPhysicalDeviceVulkanMemoryModelFeatures);
        F(VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_SCALAR_BLOCK_LAYOUT_FEATURES, VkPhysicalDeviceScalarBlockLayoutFeatures);
        F(VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_SHADER_SUBGROUP_EXTENDED_TYPES_FEATURES, VkPhysicalDeviceShaderSubgroupExtendedTypesFeatures);
        F(VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_TIMELINE_SEMAPHORE_FEATURES, VkPhysicalDeviceTimelineSemaphoreFeatures);
        F(VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_HOST_QUERY_RESET_FEATURES, VkPhysicalDeviceHostQueryResetFeatures);
        F(VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_DESCRIPTOR_INDEXING_FEATURES, VkPhysicalDeviceDescriptorIndexingFeatures);
        F(VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_SHADER_ATOMIC_INT64_FEATURES, VkPhysicalDeviceShaderAtomicInt64Features);
        F(VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_SHADER_ATOMIC_FLOAT_FEATURES_EXT, VkPhysicalDeviceShaderAtomicFloatFeaturesEXT);
        F(VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_PIPELINE_EXECUTABLE_PROPERTIES_FEATURES_KHR, VkPhysicalDevicePipelineExecutablePropertiesFeaturesKHR);
        F(VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_MAINTENANCE_5_FEATURES, VkPhysicalDeviceMaintenance5Features);
        F(VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_SHADER_INTEGER_FUNCTIONS_2_FEATURES_INTEL, VkPhysicalDeviceShaderIntegerFunctions2FeaturesINTEL);
        F(VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_FAULT_FEATURES_EXT, VkPhysicalDeviceFaultFeaturesEXT);
        F(VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_INTERNALLY_SYNCHRONIZED_QUEUES_FEATURES_KHR, VkPhysicalDeviceInternallySynchronizedQueuesFeaturesKHR);
#ifdef VK_VALVE_shader_mixed_float_dot_product
        F(VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_SHADER_MIXED_FLOAT_DOT_PRODUCT_FEATURES_VALVE, VkPhysicalDeviceShaderMixedFloatDotProductFeaturesVALVE);
#endif
#undef F
    default: return 0;
    }
}

// (set, binding) pairs the shader declares. A reused descriptor set can still hold
// stale bindings from a wider pipeline, possibly to buffers since destroyed.
std::set<std::pair<uint32_t, uint32_t>> declared_bindings(const std::vector<uint32_t> &spirv) {
    std::map<uint32_t, uint32_t> set_of, binding_of;
    for (size_t i = 5; i < spirv.size();) {
        uint32_t words = spirv[i] >> 16, opcode = spirv[i] & 0xffff;
        if (words == 0 || i + words > spirv.size()) break;
        if (opcode == 71 && words >= 4) {  // OpDecorate target decoration literal
            if (spirv[i + 2] == 34) set_of[spirv[i + 1]] = spirv[i + 3];      // DescriptorSet
            if (spirv[i + 2] == 33) binding_of[spirv[i + 1]] = spirv[i + 3];  // Binding
        }
        i += words;
    }
    std::set<std::pair<uint32_t, uint32_t>> out;
    for (auto &[id, binding] : binding_of) out.insert({set_of.count(id) ? set_of[id] : 0, binding});
    return out;
}

std::string stem(const std::string &label) {
    size_t open = label.rfind(" (");
    if (open != std::string::npos && !label.empty() && label.back() == ')') {
        std::string inner = label.substr(open + 2, label.size() - open - 3);
        if (!inner.empty() && inner.find_first_not_of("0123456789, ") == std::string::npos) return label.substr(0, open);
    }
    return label;
}

uint32_t memory_type(const Device &d, uint32_t bits, VkMemoryPropertyFlags want) {
    for (uint32_t i = 0; i < d.memory.memoryTypeCount; i++)
        if ((bits & (1u << i)) && (d.memory.memoryTypes[i].propertyFlags & want) == want) return i;
    return UINT32_MAX;
}

bool staging(Device &d, VkDeviceSize size, VkBuffer *buffer, VkDeviceMemory *memory) {
    VkBufferCreateInfo info{VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO};
    info.size = size;
    info.usage = VK_BUFFER_USAGE_TRANSFER_DST_BIT;
    if (d.CreateBuffer(d.device, &info, nullptr, buffer) != VK_SUCCESS) return false;
    VkMemoryRequirements req;
    d.GetBufferMemoryRequirements(d.device, *buffer, &req);
    VkMemoryAllocateInfo alloc{VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO};
    alloc.allocationSize = req.size;
    // Cached memory: the capture is read back byte by byte, which is very slow from
    // the uncached, write-combined host-visible memory an APU also offers.
    const VkMemoryPropertyFlags host = VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT | VK_MEMORY_PROPERTY_HOST_COHERENT_BIT;
    alloc.memoryTypeIndex = memory_type(d, req.memoryTypeBits, host | VK_MEMORY_PROPERTY_HOST_CACHED_BIT);
    if (alloc.memoryTypeIndex == UINT32_MAX) alloc.memoryTypeIndex = memory_type(d, req.memoryTypeBits, host);
    if (alloc.memoryTypeIndex == UINT32_MAX || d.AllocateMemory(d.device, &alloc, nullptr, memory) != VK_SUCCESS) return false;
    return d.BindBufferMemory(d.device, *buffer, *memory, 0) == VK_SUCCESS;
}

void barrier(Device &d, VkCommandBuffer cb, VkPipelineStageFlags src, VkAccessFlags src_access, VkPipelineStageFlags dst, VkAccessFlags dst_access) {
    VkMemoryBarrier b{VK_STRUCTURE_TYPE_MEMORY_BARRIER};
    b.srcAccessMask = src_access;
    b.dstAccessMask = dst_access;
    d.CmdPipelineBarrier(cb, src, dst, 0, 1, &b, 0, nullptr, 0, nullptr);
}

void copy_groups(Device &d, VkCommandBuffer cb, Pending &p, bool post) {
    for (auto &g : p.buffers) {
        VkBufferCopy region{g.lo, 0, g.hi - g.lo};
        d.CmdCopyBuffer(cb, g.buffer, post ? g.post : g.pre, 1, &region);
    }
}

bool write_file(const std::string &path, const void *data, size_t size) {
    FILE *f = fopen(path.c_str(), "wb");
    if (!f) return false;
    bool ok = fwrite(data, 1, size, f) == size;
    return fclose(f) == 0 && ok;
}

bool dump(Device &d, VkDeviceMemory memory, VkDeviceSize size, const std::string &path) {
    void *mapped = nullptr;
    if (d.MapMemory(d.device, memory, 0, size, 0, &mapped) != VK_SUCCESS) return false;
    bool ok = write_file(path, mapped, size);
    d.UnmapMemory(d.device, memory);
    return ok;
}

// The post-dispatch state as the byte runs that differ from the pre-dispatch state:
// records of (u64 offset, u64 length, bytes). A bound range is often far larger
// than what the dispatch writes, so this keeps a capture near the size of its inputs.
bool dump_changes(Device &d, VkDeviceMemory before, VkDeviceMemory after, VkDeviceSize size, const std::string &path, uint64_t *changed) {
    void *a = nullptr, *b = nullptr;
    if (d.MapMemory(d.device, before, 0, size, 0, &a) != VK_SUCCESS) return false;
    if (d.MapMemory(d.device, after, 0, size, 0, &b) != VK_SUCCESS) { d.UnmapMemory(d.device, before); return false; }
    const uint8_t *x = (const uint8_t *)a, *y = (const uint8_t *)b;
    FILE *f = fopen(path.c_str(), "wb");
    bool ok = f != nullptr;
    *changed = 0;
    for (uint64_t i = 0; ok && i < size;) {
        if (x[i] == y[i]) { i++; continue; }
        uint64_t start = i, last = i;
        // Coalesce runs separated by fewer than 64 unchanged bytes.
        for (uint64_t j = i; j < size && j - last < 64; j++)
            if (x[j] != y[j]) last = j;
        uint64_t length = last - start + 1;
        ok = fwrite(&start, 8, 1, f) == 1 && fwrite(&length, 8, 1, f) == 1 && fwrite(y + start, 1, length, f) == length;
        *changed += length;
        i = last + 1;
    }
    if (f) ok = fclose(f) == 0 && ok;
    d.UnmapMemory(d.device, before);
    d.UnmapMemory(d.device, after);
    return ok;
}

void finish(Pending *p) {
    Device &d = *p->device;
    std::string out = env("ROCPROF_VKCAP_OUTPUT");
    bool ok = true;
    std::vector<uint64_t> changed(p->buffers.size());
    for (size_t i = 0; i < p->buffers.size(); i++) {
        auto &g = p->buffers[i];
        ok = ok && dump(d, g.pre_mem, g.hi - g.lo, out + "/in_" + std::to_string(i) + ".bin");
        ok = ok && dump_changes(d, g.pre_mem, g.post_mem, g.hi - g.lo, out + "/out_" + std::to_string(i) + ".changes", &changed[i]);
        d.DestroyBuffer(d.device, g.pre, nullptr); d.DestroyBuffer(d.device, g.post, nullptr);
        d.FreeMemory(d.device, g.pre_mem, nullptr); d.FreeMemory(d.device, g.post_mem, nullptr);
    }
    const Pipeline &pl = p->pipeline;
    ok = ok && write_file(out + "/shader.spv", pl.spirv.data(), pl.spirv.size() * 4);
    ok = ok && write_file(out + "/spec.bin", pl.spec_data.data(), pl.spec_data.size());
    ok = ok && write_file(out + "/push.bin", p->push.data(), p->push.size());
    if (!d.features10.empty()) ok = ok && write_file(out + "/features10.bin", d.features10.data(), d.features10.size());
    for (size_t i = 0; i < d.features.size(); i++)
        ok = ok && write_file(out + "/feature_" + std::to_string(i) + ".bin", d.features[i].second.data(), d.features[i].second.size());
    FILE *m = fopen((out + "/capture.txt").c_str(), "w");
    if (!m || !ok) { if (m) fclose(m); fail("could not write the capture files"); return; }
    fprintf(m, "version 1\n");
    fprintf(m, "device %u %u %u %s\n", d.properties.vendorID, d.properties.deviceID, d.properties.driverVersion, d.properties.deviceName);
    fprintf(m, "api_version %u\n", d.api_version);
    fprintf(m, "name %s\n", p->name.c_str());
    for (auto &l : p->labels) fprintf(m, "label %s\n", l.c_str());
    fprintf(m, "entry %s\n", pl.entry.c_str());
    fprintf(m, "stage_flags %u\nrequired_subgroup_size %u\npipeline_flags %u\n", pl.stage_flags, pl.required_subgroup_size, pl.create_flags);
    fprintf(m, "spirv shader.spv %zu\n", pl.spirv.size() * 4);
    fprintf(m, "spec_data spec.bin %zu\n", pl.spec_data.size());
    for (auto &e : pl.spec) fprintf(m, "spec %u %u %zu\n", e.constantID, e.offset, e.size);
    fprintf(m, "push push.bin %zu\n", p->push.size());
    for (auto &r : pipeline_layouts[pl.layout].push) fprintf(m, "push_range %u %u %u\n", r.stageFlags, r.offset, r.size);
    for (auto &[set, bindings] : p->set_layouts)
        for (auto &b : bindings) fprintf(m, "set_layout %u %u %u %u\n", set, b.binding, b.type, b.count);
    fprintf(m, "groups %u %u %u\n", p->groups[0], p->groups[1], p->groups[2]);
    for (size_t i = 0; i < p->buffers.size(); i++)
        fprintf(m, "buffer %zu %llu in_%zu.bin out_%zu.changes %llu\n", i, (unsigned long long)(p->buffers[i].hi - p->buffers[i].lo), i, i, (unsigned long long)changed[i]);
    for (auto &b : p->bindings)
        fprintf(m, "bind %u %u %u %u %zu %llu %llu\n", b.set, b.binding, b.element, b.type, b.group, (unsigned long long)b.offset, (unsigned long long)b.range);
    for (auto &e : d.extensions) fprintf(m, "extension %s\n", e.c_str());
    if (!d.features10.empty()) fprintf(m, "features10 features10.bin %zu\n", d.features10.size());
    for (size_t i = 0; i < d.features.size(); i++) fprintf(m, "feature %u feature_%zu.bin %zu\n", d.features[i].first, i, d.features[i].second.size());
    for (auto t : d.unknown_features) fprintf(m, "unknown_feature %u\n", t);
    fclose(m);
    write_file(out + "/capture_complete", "", 0);
    finished = true;
}

// ---- instance and device ------------------------------------------------------

VKAPI_ATTR VkResult VKAPI_CALL CreateInstance(const VkInstanceCreateInfo *info, const VkAllocationCallbacks *alloc, VkInstance *instance) {
    auto *chain = (VkLayerInstanceCreateInfo *)info->pNext;
    while (chain && !(chain->sType == VK_STRUCTURE_TYPE_LOADER_INSTANCE_CREATE_INFO && chain->function == VK_LAYER_LINK_INFO))
        chain = (VkLayerInstanceCreateInfo *)chain->pNext;
    if (!chain) return VK_ERROR_INITIALIZATION_FAILED;
    PFN_vkGetInstanceProcAddr gipa = chain->u.pLayerInfo->pfnNextGetInstanceProcAddr;
    chain->u.pLayerInfo = chain->u.pLayerInfo->pNext;
    auto create = (PFN_vkCreateInstance)gipa(VK_NULL_HANDLE, "vkCreateInstance");
    VkResult r = create(info, alloc, instance);
    if (r != VK_SUCCESS) return r;
    auto *data = new Instance;
    data->gipa = gipa;
    data->destroy = (PFN_vkDestroyInstance)gipa(*instance, "vkDestroyInstance");
    data->memory_properties = (PFN_vkGetPhysicalDeviceMemoryProperties)gipa(*instance, "vkGetPhysicalDeviceMemoryProperties");
    data->properties = (PFN_vkGetPhysicalDeviceProperties)gipa(*instance, "vkGetPhysicalDeviceProperties");
    if (info->pApplicationInfo) data->api_version = info->pApplicationInfo->apiVersion;
    std::lock_guard<std::mutex> g(lock);
    instances[key(*instance)] = data;
    return VK_SUCCESS;
}

VKAPI_ATTR void VKAPI_CALL DestroyInstance(VkInstance instance, const VkAllocationCallbacks *alloc) {
    Instance *data;
    {
        std::lock_guard<std::mutex> g(lock);
        data = instances[key(instance)];
        instances.erase(key(instance));
    }
    data->destroy(instance, alloc);
    delete data;
}

VKAPI_ATTR VkResult VKAPI_CALL CreateDevice(VkPhysicalDevice physical, const VkDeviceCreateInfo *info, const VkAllocationCallbacks *alloc, VkDevice *device) {
    auto *chain = (VkLayerDeviceCreateInfo *)info->pNext;
    while (chain && !(chain->sType == VK_STRUCTURE_TYPE_LOADER_DEVICE_CREATE_INFO && chain->function == VK_LAYER_LINK_INFO))
        chain = (VkLayerDeviceCreateInfo *)chain->pNext;
    if (!chain) return VK_ERROR_INITIALIZATION_FAILED;
    PFN_vkGetInstanceProcAddr gipa = chain->u.pLayerInfo->pfnNextGetInstanceProcAddr;
    PFN_vkGetDeviceProcAddr gdpa = chain->u.pLayerInfo->pfnNextGetDeviceProcAddr;
    chain->u.pLayerInfo = chain->u.pLayerInfo->pNext;
    Instance *inst;
    {
        std::lock_guard<std::mutex> g(lock);
        inst = instances[key(physical)];
    }
    auto create = (PFN_vkCreateDevice)gipa(VK_NULL_HANDLE, "vkCreateDevice");
    VkResult r = create(physical, info, alloc, device);
    if (r != VK_SUCCESS) return r;
    auto *d = new Device;
    d->device = *device;
    d->gdpa = gdpa;
    d->api_version = inst->api_version;
    inst->memory_properties(physical, &d->memory);
    inst->properties(physical, &d->properties);
    for (uint32_t i = 0; i < info->enabledExtensionCount; i++) d->extensions.push_back(info->ppEnabledExtensionNames[i]);
    if (info->pEnabledFeatures)
        d->features10.assign((const uint8_t *)info->pEnabledFeatures, (const uint8_t *)info->pEnabledFeatures + sizeof(VkPhysicalDeviceFeatures));
    for (auto *s = (const VkBaseInStructure *)info->pNext; s; s = s->pNext) {
        if (s->sType == VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_FEATURES_2) {
            auto *f2 = (const VkPhysicalDeviceFeatures2 *)s;
            d->features10.assign((const uint8_t *)&f2->features, (const uint8_t *)&f2->features + sizeof(VkPhysicalDeviceFeatures));
        } else if (size_t size = feature_size(s->sType)) {
            const uint8_t *body = (const uint8_t *)s + sizeof(VkBaseInStructure);
            d->features.push_back({(uint32_t)s->sType, std::vector<uint8_t>(body, body + size - sizeof(VkBaseInStructure))});
        } else if (s->sType != VK_STRUCTURE_TYPE_LOADER_DEVICE_CREATE_INFO) {
            d->unknown_features.push_back((uint32_t)s->sType);
        }
    }
#define FN(name) d->name = (PFN_vk##name)gdpa(*device, "vk" #name)
    FN(DestroyDevice); FN(CreateBuffer); FN(DestroyBuffer); FN(GetBufferMemoryRequirements);
    FN(AllocateMemory); FN(FreeMemory); FN(BindBufferMemory); FN(MapMemory); FN(UnmapMemory);
    FN(CreateShaderModule); FN(CreateComputePipelines); FN(CreatePipelineLayout);
    FN(CreateDescriptorSetLayout); FN(UpdateDescriptorSets); FN(BeginCommandBuffer);
    FN(CmdBindPipeline); FN(CmdBindDescriptorSets); FN(CmdPushConstants); FN(CmdDispatch);
    FN(CmdPipelineBarrier); FN(CmdCopyBuffer); FN(QueueSubmit); FN(QueueSubmit2);
    FN(QueueWaitIdle); FN(SetDebugUtilsObjectNameEXT); FN(CmdBeginDebugUtilsLabelEXT);
    FN(CmdEndDebugUtilsLabelEXT); FN(AllocateDescriptorSets);
#undef FN
    if (!d->QueueSubmit2) d->QueueSubmit2 = (PFN_vkQueueSubmit2)gdpa(*device, "vkQueueSubmit2KHR");
    std::lock_guard<std::mutex> g(lock);
    devices[key(*device)] = d;
    return VK_SUCCESS;
}

Device *device_of(void *k) {
    std::lock_guard<std::mutex> g(lock);
    auto it = devices.find(k);
    return it == devices.end() ? nullptr : it->second;
}

VKAPI_ATTR void VKAPI_CALL DestroyDevice(VkDevice device, const VkAllocationCallbacks *alloc) {
    Device *d = device_of(key(device));
    {
        std::lock_guard<std::mutex> g(lock);
        devices.erase(key(device));
    }
    d->DestroyDevice(device, alloc);
    delete d;
}

// ---- object tracking -----------------------------------------------------------

VKAPI_ATTR VkResult VKAPI_CALL CreateBuffer(VkDevice device, const VkBufferCreateInfo *info, const VkAllocationCallbacks *alloc, VkBuffer *buffer) {
    Device *d = device_of(key(device));
    VkBufferCreateInfo copy = *info;
    // Bound ranges are copied out at the captured dispatch.
    if (active() && (copy.usage & (VK_BUFFER_USAGE_STORAGE_BUFFER_BIT | VK_BUFFER_USAGE_UNIFORM_BUFFER_BIT)))
        copy.usage |= VK_BUFFER_USAGE_TRANSFER_SRC_BIT;
    VkResult r = d->CreateBuffer(device, &copy, alloc, buffer);
    if (r == VK_SUCCESS) {
        std::lock_guard<std::mutex> g(lock);
        buffer_sizes[*buffer] = info->size;
    }
    return r;
}

VKAPI_ATTR VkResult VKAPI_CALL CreateShaderModule(VkDevice device, const VkShaderModuleCreateInfo *info, const VkAllocationCallbacks *alloc, VkShaderModule *module) {
    Device *d = device_of(key(device));
    VkResult r = d->CreateShaderModule(device, info, alloc, module);
    if (r == VK_SUCCESS) {
        std::lock_guard<std::mutex> g(lock);
        modules[*module].assign(info->pCode, info->pCode + info->codeSize / 4);
    }
    return r;
}

VKAPI_ATTR VkResult VKAPI_CALL CreateDescriptorSetLayout(VkDevice device, const VkDescriptorSetLayoutCreateInfo *info, const VkAllocationCallbacks *alloc, VkDescriptorSetLayout *layout) {
    Device *d = device_of(key(device));
    VkResult r = d->CreateDescriptorSetLayout(device, info, alloc, layout);
    if (r == VK_SUCCESS) {
        std::vector<SetLayoutBinding> b;
        for (uint32_t i = 0; i < info->bindingCount; i++)
            b.push_back({info->pBindings[i].binding, (uint32_t)info->pBindings[i].descriptorType, info->pBindings[i].descriptorCount});
        std::lock_guard<std::mutex> g(lock);
        set_layouts[*layout] = std::move(b);
    }
    return r;
}

VKAPI_ATTR VkResult VKAPI_CALL CreatePipelineLayout(VkDevice device, const VkPipelineLayoutCreateInfo *info, const VkAllocationCallbacks *alloc, VkPipelineLayout *layout) {
    Device *d = device_of(key(device));
    VkResult r = d->CreatePipelineLayout(device, info, alloc, layout);
    if (r == VK_SUCCESS) {
        PipelineLayout pl;
        pl.sets.assign(info->pSetLayouts, info->pSetLayouts + info->setLayoutCount);
        pl.push.assign(info->pPushConstantRanges, info->pPushConstantRanges + info->pushConstantRangeCount);
        std::lock_guard<std::mutex> g(lock);
        pipeline_layouts[*layout] = std::move(pl);
    }
    return r;
}

VKAPI_ATTR VkResult VKAPI_CALL CreateComputePipelines(VkDevice device, VkPipelineCache cache, uint32_t count, const VkComputePipelineCreateInfo *infos, const VkAllocationCallbacks *alloc, VkPipeline *out) {
    Device *d = device_of(key(device));
    VkResult r = d->CreateComputePipelines(device, cache, count, infos, alloc, out);
    if (r != VK_SUCCESS) return r;
    std::lock_guard<std::mutex> g(lock);
    for (uint32_t i = 0; i < count; i++) {
        const VkPipelineShaderStageCreateInfo &stage = infos[i].stage;
        Pipeline p;
        p.device = d;
        auto it = modules.find(stage.module);
        if (it != modules.end()) p.spirv = it->second;
        for (auto *s = (const VkBaseInStructure *)stage.pNext; s; s = s->pNext) {
            if (s->sType == VK_STRUCTURE_TYPE_SHADER_MODULE_CREATE_INFO) {
                auto *m = (const VkShaderModuleCreateInfo *)s;
                p.spirv.assign(m->pCode, m->pCode + m->codeSize / 4);
            } else if (s->sType == VK_STRUCTURE_TYPE_PIPELINE_SHADER_STAGE_REQUIRED_SUBGROUP_SIZE_CREATE_INFO) {
                p.required_subgroup_size = ((const VkPipelineShaderStageRequiredSubgroupSizeCreateInfo *)s)->requiredSubgroupSize;
            }
        }
        p.entry = stage.pName;
        p.stage_flags = stage.flags;
        p.create_flags = infos[i].flags;
        p.layout = infos[i].layout;
        if (const VkSpecializationInfo *spec = stage.pSpecializationInfo) {
            p.spec.assign(spec->pMapEntries, spec->pMapEntries + spec->mapEntryCount);
            p.spec_data.assign((const uint8_t *)spec->pData, (const uint8_t *)spec->pData + spec->dataSize);
        }
        pipelines[out[i]] = std::move(p);
    }
    return r;
}

VKAPI_ATTR VkResult VKAPI_CALL AllocateDescriptorSets(VkDevice device, const VkDescriptorSetAllocateInfo *info, VkDescriptorSet *sets) {
    Device *d = device_of(key(device));
    VkResult r = d->AllocateDescriptorSets(device, info, sets);
    if (r == VK_SUCCESS) {
        std::lock_guard<std::mutex> g(lock);
        for (uint32_t i = 0; i < info->descriptorSetCount; i++) {
            set_layout_of[sets[i]] = info->pSetLayouts[i];
            descriptor_sets.erase(sets[i]);
        }
    }
    return r;
}

// Maps descriptor j of an update that starts at (binding, element) to the slot it
// lands in: an update longer than its binding continues into the next binding.
uint64_t slot_of(VkDescriptorSet set, uint32_t binding, uint32_t element, uint32_t j) {
    auto layout = set_layout_of.find(set);
    if (layout == set_layout_of.end()) return ((uint64_t)binding << 32) | (element + j);
    std::vector<SetLayoutBinding> bindings = set_layouts[layout->second];
    std::sort(bindings.begin(), bindings.end(), [](auto &a, auto &b) { return a.binding < b.binding; });
    auto it = std::find_if(bindings.begin(), bindings.end(), [&](auto &b) { return b.binding == binding; });
    uint32_t e = element + j;
    while (it != bindings.end() && e >= it->count) { e -= it->count; ++it; while (it != bindings.end() && it->count == 0) ++it; }
    if (it == bindings.end()) return ((uint64_t)binding << 32) | (element + j);
    return ((uint64_t)it->binding << 32) | e;
}

VKAPI_ATTR void VKAPI_CALL UpdateDescriptorSets(VkDevice device, uint32_t writes, const VkWriteDescriptorSet *w, uint32_t copies, const VkCopyDescriptorSet *c) {
    Device *d = device_of(key(device));
    {
        std::lock_guard<std::mutex> g(lock);
        for (uint32_t i = 0; i < writes; i++) {
            auto &set = descriptor_sets[w[i].dstSet];
            for (uint32_t j = 0; j < w[i].descriptorCount; j++) {
                uint64_t slot = slot_of(w[i].dstSet, w[i].dstBinding, w[i].dstArrayElement, j);
                if (w[i].pBufferInfo)
                    set[slot] = {(uint32_t)w[i].descriptorType, w[i].pBufferInfo[j].buffer, w[i].pBufferInfo[j].offset, w[i].pBufferInfo[j].range};
                else
                    set[slot] = {(uint32_t)w[i].descriptorType, VK_NULL_HANDLE, 0, 0};
            }
        }
        for (uint32_t i = 0; i < copies; i++) {
            auto &src = descriptor_sets[c[i].srcSet];
            auto &dst = descriptor_sets[c[i].dstSet];
            for (uint32_t j = 0; j < c[i].descriptorCount; j++) {
                auto it = src.find(slot_of(c[i].srcSet, c[i].srcBinding, c[i].srcArrayElement, j));
                if (it != src.end()) dst[slot_of(c[i].dstSet, c[i].dstBinding, c[i].dstArrayElement, j)] = it->second;
            }
        }
    }
    d->UpdateDescriptorSets(device, writes, w, copies, c);
}

VKAPI_ATTR VkResult VKAPI_CALL SetDebugUtilsObjectNameEXT(VkDevice device, const VkDebugUtilsObjectNameInfoEXT *info) {
    Device *d = device_of(key(device));
    if (info->pObjectName) {
        std::lock_guard<std::mutex> g(lock);
        names[info->objectHandle] = info->pObjectName;
    }
    return d->SetDebugUtilsObjectNameEXT ? d->SetDebugUtilsObjectNameEXT(device, info) : VK_SUCCESS;
}

// ---- command recording ---------------------------------------------------------

VKAPI_ATTR VkResult VKAPI_CALL BeginCommandBuffer(VkCommandBuffer cb, const VkCommandBufferBeginInfo *info) {
    Device *d = device_of(key(cb));
    {
        std::lock_guard<std::mutex> g(lock);
        auto &s = commands[cb];
        Pending *keep = s.pending;
        s = CommandState{};
        s.device = d;
        s.pending = keep;
    }
    return d->BeginCommandBuffer(cb, info);
}

VKAPI_ATTR void VKAPI_CALL CmdBindPipeline(VkCommandBuffer cb, VkPipelineBindPoint point, VkPipeline pipeline) {
    Device *d = device_of(key(cb));
    if (point == VK_PIPELINE_BIND_POINT_COMPUTE) {
        std::lock_guard<std::mutex> g(lock);
        commands[cb].pipeline = pipeline;
    }
    d->CmdBindPipeline(cb, point, pipeline);
}

VKAPI_ATTR void VKAPI_CALL CmdBindDescriptorSets(VkCommandBuffer cb, VkPipelineBindPoint point, VkPipelineLayout layout, uint32_t first, uint32_t count, const VkDescriptorSet *sets, uint32_t dynamic_count, const uint32_t *dynamic) {
    Device *d = device_of(key(cb));
    if (point == VK_PIPELINE_BIND_POINT_COMPUTE) {
        std::lock_guard<std::mutex> g(lock);
        auto &s = commands[cb];
        uint32_t used = 0;
        for (uint32_t i = 0; i < count; i++) {
            std::vector<uint32_t> offsets;
            for (auto &[slot, b] : descriptor_sets[sets[i]])
                if (b.type == VK_DESCRIPTOR_TYPE_STORAGE_BUFFER_DYNAMIC || b.type == VK_DESCRIPTOR_TYPE_UNIFORM_BUFFER_DYNAMIC)
                    if (used < dynamic_count) offsets.push_back(dynamic[used++]);
            s.sets[first + i] = {sets[i], offsets};
        }
    }
    d->CmdBindDescriptorSets(cb, point, layout, first, count, sets, dynamic_count, dynamic);
}

VKAPI_ATTR void VKAPI_CALL CmdPushConstants(VkCommandBuffer cb, VkPipelineLayout layout, VkShaderStageFlags stages, uint32_t offset, uint32_t size, const void *values) {
    Device *d = device_of(key(cb));
    {
        std::lock_guard<std::mutex> g(lock);
        if (offset + size <= sizeof(CommandState::push)) memcpy(commands[cb].push + offset, values, size);
    }
    d->CmdPushConstants(cb, layout, stages, offset, size, values);
}

VKAPI_ATTR void VKAPI_CALL CmdBeginDebugUtilsLabelEXT(VkCommandBuffer cb, const VkDebugUtilsLabelEXT *label) {
    Device *d = device_of(key(cb));
    {
        std::lock_guard<std::mutex> g(lock);
        commands[cb].labels.push_back(label->pLabelName ? label->pLabelName : "");
    }
    if (d->CmdBeginDebugUtilsLabelEXT) d->CmdBeginDebugUtilsLabelEXT(cb, label);
}

VKAPI_ATTR void VKAPI_CALL CmdEndDebugUtilsLabelEXT(VkCommandBuffer cb) {
    Device *d = device_of(key(cb));
    {
        std::lock_guard<std::mutex> g(lock);
        auto &labels = commands[cb].labels;
        if (!labels.empty()) labels.pop_back();
    }
    if (d->CmdEndDebugUtilsLabelEXT) d->CmdEndDebugUtilsLabelEXT(cb);
}

// Returns the capture to record around this dispatch, or null. Called under the lock.
Pending *select(VkCommandBuffer cb, uint32_t x, uint32_t y, uint32_t z) {
    if (finished || armed || !active()) return nullptr;
    auto &s = commands[cb];
    auto pit = pipelines.find(s.pipeline);
    if (pit == pipelines.end()) return nullptr;
    const char *want = env("ROCPROF_VKCAP_NAME");
    std::string name = names.count((uint64_t)s.pipeline) ? names[(uint64_t)s.pipeline] : "";
    std::string label = s.labels.empty() ? "" : s.labels.back();
    if (want && name != want && stem(label) != want && label != want) return nullptr;
    if (const char *groups = env("ROCPROF_VKCAP_GROUPS")) {
        unsigned gx = 0, gy = 0, gz = 0;
        if (sscanf(groups, "%u,%u,%u", &gx, &gy, &gz) == 3 && (gx != x || gy != y || gz != z)) return nullptr;
    }
    uint64_t occurrence = env("ROCPROF_VKCAP_OCCURRENCE") ? strtoull(env("ROCPROF_VKCAP_OCCURRENCE"), nullptr, 10) : 0;
    if (matched++ != occurrence) return nullptr;
    armed = true;
    Device &d = *s.device;
    auto *p = new Pending;
    p->device = &d;
    p->pipeline = pit->second;
    p->name = name.empty() ? stem(label) : name;
    p->label = label;
    p->labels = s.labels;
    p->groups[0] = x; p->groups[1] = y; p->groups[2] = z;
    const PipelineLayout &layout = pipeline_layouts[p->pipeline.layout];
    uint32_t push_size = 0;
    for (auto &r : layout.push) push_size = std::max(push_size, r.offset + r.size);
    p->push.assign(s.push, s.push + std::min<uint32_t>(push_size, sizeof(s.push)));
    std::map<VkBuffer, size_t> group_of;
    auto declared = declared_bindings(p->pipeline.spirv);
    if (declared.empty()) { fail("the pipeline's SPIR-V declares no descriptor bindings"); delete p; return nullptr; }
    for (uint32_t set = 0; set < layout.sets.size(); set++) {
        const auto &bindings = set_layouts[layout.sets[set]];
        p->set_layouts.push_back({set, bindings});
        auto bound = s.sets.find(set);
        if (bound == s.sets.end()) { fail("descriptor set " + std::to_string(set) + " is not bound at the captured dispatch"); delete p; return nullptr; }
        const auto &contents = descriptor_sets[bound->second.first];
        size_t dynamic = 0;
        for (auto &[slot, b] : contents) {
            uint32_t binding = slot >> 32, element = (uint32_t)slot;
            if (!declared.count({set, binding})) continue;
            bool is_dynamic = b.type == VK_DESCRIPTOR_TYPE_STORAGE_BUFFER_DYNAMIC || b.type == VK_DESCRIPTOR_TYPE_UNIFORM_BUFFER_DYNAMIC;
            if (b.buffer == VK_NULL_HANDLE ||
                !(b.type == VK_DESCRIPTOR_TYPE_STORAGE_BUFFER || b.type == VK_DESCRIPTOR_TYPE_UNIFORM_BUFFER || is_dynamic)) {
                fail("binding " + std::to_string(binding) + " holds a descriptor type other than a buffer; only buffer descriptors can be captured");
                delete p;
                return nullptr;
            }
            VkDeviceSize offset = b.offset + (is_dynamic && dynamic < bound->second.second.size() ? bound->second.second[dynamic++] : 0);
            VkDeviceSize size = buffer_sizes.count(b.buffer) ? buffer_sizes[b.buffer] : 0;
            VkDeviceSize range = b.range == VK_WHOLE_SIZE ? size - offset : b.range;
            auto it = group_of.find(b.buffer);
            if (it == group_of.end()) {
                it = group_of.emplace(b.buffer, p->buffers.size()).first;
                p->buffers.push_back({b.buffer, offset, offset + range});
            }
            Group &grp = p->buffers[it->second];
            grp.lo = std::min(grp.lo, offset);
            grp.hi = std::max(grp.hi, offset + range);
            p->bindings.push_back({set, binding, element, b.type, it->second, offset, range});
        }
    }
    for (auto &b : p->bindings) b.offset -= p->buffers[b.group].lo;
    for (auto &g : p->buffers)
        if (!staging(d, g.hi - g.lo, &g.pre, &g.pre_mem) || !staging(d, g.hi - g.lo, &g.post, &g.post_mem)) {
            fail("could not allocate host-visible memory for the captured buffers");
            delete p;
            return nullptr;
        }
    s.pending = p;
    return p;
}

VKAPI_ATTR void VKAPI_CALL CmdDispatch(VkCommandBuffer cb, uint32_t x, uint32_t y, uint32_t z) {
    Device *d = device_of(key(cb));
    Pending *p;
    {
        std::lock_guard<std::mutex> g(lock);
        p = select(cb, x, y, z);
    }
    if (!p) { d->CmdDispatch(cb, x, y, z); return; }
    const VkAccessFlags any = VK_ACCESS_MEMORY_READ_BIT | VK_ACCESS_MEMORY_WRITE_BIT;
    barrier(*d, cb, VK_PIPELINE_STAGE_ALL_COMMANDS_BIT, VK_ACCESS_MEMORY_WRITE_BIT, VK_PIPELINE_STAGE_TRANSFER_BIT, VK_ACCESS_TRANSFER_READ_BIT);
    copy_groups(*d, cb, *p, false);
    barrier(*d, cb, VK_PIPELINE_STAGE_TRANSFER_BIT, VK_ACCESS_TRANSFER_READ_BIT | VK_ACCESS_TRANSFER_WRITE_BIT, VK_PIPELINE_STAGE_COMPUTE_SHADER_BIT, VK_ACCESS_SHADER_READ_BIT | VK_ACCESS_SHADER_WRITE_BIT);
    d->CmdDispatch(cb, x, y, z);
    barrier(*d, cb, VK_PIPELINE_STAGE_COMPUTE_SHADER_BIT, VK_ACCESS_SHADER_WRITE_BIT, VK_PIPELINE_STAGE_TRANSFER_BIT, VK_ACCESS_TRANSFER_READ_BIT);
    copy_groups(*d, cb, *p, true);
    barrier(*d, cb, VK_PIPELINE_STAGE_TRANSFER_BIT, VK_ACCESS_TRANSFER_READ_BIT | VK_ACCESS_TRANSFER_WRITE_BIT, VK_PIPELINE_STAGE_ALL_COMMANDS_BIT | VK_PIPELINE_STAGE_HOST_BIT, any | VK_ACCESS_HOST_READ_BIT);
}

void after_submit(VkQueue queue, Device *d, const std::vector<VkCommandBuffer> &cbs) {
    std::vector<Pending *> ready;
    {
        std::lock_guard<std::mutex> g(lock);
        for (auto cb : cbs) {
            auto it = commands.find(cb);
            if (it != commands.end() && it->second.pending) {
                ready.push_back(it->second.pending);
                it->second.pending = nullptr;
            }
        }
    }
    if (ready.empty()) return;
    d->QueueWaitIdle(queue);
    std::lock_guard<std::mutex> g(lock);
    for (Pending *p : ready) { finish(p); delete p; }
}

VKAPI_ATTR VkResult VKAPI_CALL QueueSubmit(VkQueue queue, uint32_t count, const VkSubmitInfo *submits, VkFence fence) {
    Device *d = device_of(key(queue));
    VkResult r = d->QueueSubmit(queue, count, submits, fence);
    if (r == VK_SUCCESS && armed && !finished) {
        std::vector<VkCommandBuffer> cbs;
        for (uint32_t i = 0; i < count; i++) cbs.insert(cbs.end(), submits[i].pCommandBuffers, submits[i].pCommandBuffers + submits[i].commandBufferCount);
        after_submit(queue, d, cbs);
    }
    return r;
}

VKAPI_ATTR VkResult VKAPI_CALL QueueSubmit2(VkQueue queue, uint32_t count, const VkSubmitInfo2 *submits, VkFence fence) {
    Device *d = device_of(key(queue));
    VkResult r = d->QueueSubmit2(queue, count, submits, fence);
    if (r == VK_SUCCESS && armed && !finished) {
        std::vector<VkCommandBuffer> cbs;
        for (uint32_t i = 0; i < count; i++)
            for (uint32_t j = 0; j < submits[i].commandBufferInfoCount; j++) cbs.push_back(submits[i].pCommandBufferInfos[j].commandBuffer);
        after_submit(queue, d, cbs);
    }
    return r;
}

// ---- dispatch ---------------------------------------------------------------------

PFN_vkVoidFunction device_hook(const char *name) {
#define H(fn) if (!strcmp(name, "vk" #fn)) return (PFN_vkVoidFunction)fn
    H(DestroyDevice); H(CreateBuffer); H(CreateShaderModule); H(CreateDescriptorSetLayout);
    H(CreatePipelineLayout); H(CreateComputePipelines); H(UpdateDescriptorSets); H(AllocateDescriptorSets);
    H(SetDebugUtilsObjectNameEXT); H(BeginCommandBuffer); H(CmdBindPipeline);
    H(CmdBindDescriptorSets); H(CmdPushConstants); H(CmdBeginDebugUtilsLabelEXT);
    H(CmdEndDebugUtilsLabelEXT); H(CmdDispatch); H(QueueSubmit); H(QueueSubmit2);
    if (!strcmp(name, "vkQueueSubmit2KHR")) return (PFN_vkVoidFunction)QueueSubmit2;
#undef H
    return nullptr;
}

}  // namespace

extern "C" VKAPI_ATTR PFN_vkVoidFunction VKAPI_CALL vkcap_GetDeviceProcAddr(VkDevice device, const char *name);

extern "C" VKAPI_ATTR PFN_vkVoidFunction VKAPI_CALL vkcap_GetInstanceProcAddr(VkInstance instance, const char *name) {
    if (!strcmp(name, "vkGetInstanceProcAddr")) return (PFN_vkVoidFunction)vkcap_GetInstanceProcAddr;
    if (!strcmp(name, "vkGetDeviceProcAddr")) return (PFN_vkVoidFunction)vkcap_GetDeviceProcAddr;
    if (!strcmp(name, "vkCreateInstance")) return (PFN_vkVoidFunction)CreateInstance;
    if (!strcmp(name, "vkDestroyInstance")) return (PFN_vkVoidFunction)DestroyInstance;
    if (!strcmp(name, "vkCreateDevice")) return (PFN_vkVoidFunction)CreateDevice;
    if (!instance) return nullptr;
    if (PFN_vkVoidFunction hook = device_hook(name)) return hook;
    Instance *data;
    {
        std::lock_guard<std::mutex> g(lock);
        data = instances[key(instance)];
    }
    return data ? data->gipa(instance, name) : nullptr;
}

extern "C" VKAPI_ATTR PFN_vkVoidFunction VKAPI_CALL vkcap_GetDeviceProcAddr(VkDevice device, const char *name) {
    if (!strcmp(name, "vkGetDeviceProcAddr")) return (PFN_vkVoidFunction)vkcap_GetDeviceProcAddr;
    Device *d = device_of(key(device));
    if (!d) return nullptr;
    PFN_vkVoidFunction next = d->gdpa(device, name);
    if (!next) return nullptr;  // not supported by the device: do not advertise a hook
    if (PFN_vkVoidFunction hook = device_hook(name)) return hook;
    return next;
}

EXPORT VKAPI_ATTR VkResult VKAPI_CALL vkNegotiateLoaderLayerInterfaceVersion(VkNegotiateLayerInterface *interface) {
    if (interface->loaderLayerInterfaceVersion < 2) return VK_ERROR_INITIALIZATION_FAILED;
    interface->loaderLayerInterfaceVersion = 2;
    interface->pfnGetInstanceProcAddr = vkcap_GetInstanceProcAddr;
    interface->pfnGetDeviceProcAddr = vkcap_GetDeviceProcAddr;
    interface->pfnGetPhysicalDeviceProcAddr = nullptr;
    return VK_SUCCESS;
}
