// Replays one dispatch captured by VK_LAYER_ROCPROF_capture in isolation.
//
//   vkreplay CAPTURE_DIR [--spirv FILE] [--iterations N] [--no-validate]
//            [--isa FILE] [--dump-outputs DIR]
//            [--mode stateless|sustained] [--memory device|app|gtt|auto] [--flush-mb N] [--counters]
//
// Each timed iteration restores every captured input from a pristine copy, so a
// dispatch that writes its own inputs is measured on the same data every time.
// --mode, --memory and --flush-mb change the conditions the dispatch runs under,
// to compare an isolated timing with the application's. --counters adds the
// driver's performance counters (VK_KHR_performance_query), including GPU cycles.
// Validation replays once more and compares every bound buffer byte for byte with
// what the application's own dispatch left there. Prints one JSON object.
#include <vulkan/vulkan.h>

#include <algorithm>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <map>
#include <numeric>
#include <sstream>
#include <stdexcept>
#include <string>
#include <vector>

namespace {

struct Spec { uint32_t id, offset; size_t size; };
struct SetLayout { uint32_t set, binding, type, count; };
struct Bind { uint32_t set, binding, element, type; size_t group; uint64_t offset, range; };
struct BufferGroup { size_t index; uint64_t size; std::string in, out; uint64_t changed = 0; };
struct Feature { uint32_t stype; std::string file; size_t size; };
struct Capture {
    std::string dir, name, entry, spirv, spec_file, push_file, features10;
    uint32_t vendor = 0, device = 0, api = VK_API_VERSION_1_3, stage_flags = 0, subgroup = 0, pipeline_flags = 0;
    uint32_t groups[3] = {1, 1, 1};
    std::vector<Spec> spec;
    std::vector<VkPushConstantRange> push_ranges;
    std::vector<SetLayout> layouts;
    std::vector<BufferGroup> buffers;
    std::vector<Bind> binds;
    std::vector<std::string> extensions;
    std::vector<Feature> features;
    std::vector<uint32_t> unknown_features;
};

[[noreturn]] void die(const std::string &m) { throw std::runtime_error(m); }
void check(VkResult r, const char *what) { if (r != VK_SUCCESS) die(std::string(what) + " failed (" + std::to_string(r) + ")"); }

std::vector<uint8_t> slurp(const std::string &path) {
    std::ifstream f(path, std::ios::binary);
    if (!f) die("cannot read " + path);
    return std::vector<uint8_t>((std::istreambuf_iterator<char>(f)), std::istreambuf_iterator<char>());
}

// The application's post-dispatch contents: the input with its recorded changes applied.
std::vector<uint8_t> expected_output(const Capture &c, const BufferGroup &g, std::vector<uint8_t> input) {
    auto changes = slurp(c.dir + "/" + g.out);
    for (size_t at = 0; at + 16 <= changes.size();) {
        uint64_t offset, length;
        memcpy(&offset, &changes[at], 8);
        memcpy(&length, &changes[at + 8], 8);
        at += 16;
        if (offset + length > input.size() || at + length > changes.size()) die("corrupt change record in " + g.out);
        memcpy(&input[offset], &changes[at], length);
        at += length;
    }
    return input;
}

std::string json_string(const std::string &s) {
    std::string out = "\"";
    for (char c : s) {
        if (c == '"' || c == '\\') { out += '\\'; out += c; }
        else if ((unsigned char)c < 0x20) { char b[8]; snprintf(b, sizeof b, "\\u%04x", c); out += b; }
        else out += c;
    }
    return out + "\"";
}

Capture load(const std::string &dir) {
    Capture c;
    c.dir = dir;
    std::ifstream f(dir + "/capture.txt");
    if (!f) die("no capture.txt in " + dir);
    if (!std::ifstream(dir + "/capture_complete")) die("capture did not complete");
    std::string line;
    while (std::getline(f, line)) {
        std::istringstream in(line);
        std::string k;
        in >> k;
        if (k == "device") in >> c.vendor >> c.device;
        else if (k == "api_version") in >> c.api;
        else if (k == "name") { std::getline(in, c.name); if (!c.name.empty() && c.name[0] == ' ') c.name.erase(0, 1); }
        else if (k == "entry") in >> c.entry;
        else if (k == "stage_flags") in >> c.stage_flags;
        else if (k == "required_subgroup_size") in >> c.subgroup;
        else if (k == "pipeline_flags") in >> c.pipeline_flags;
        else if (k == "spirv") in >> c.spirv;
        else if (k == "spec_data") in >> c.spec_file;
        else if (k == "spec") { Spec s; in >> s.id >> s.offset >> s.size; c.spec.push_back(s); }
        else if (k == "push") in >> c.push_file;
        else if (k == "push_range") { VkPushConstantRange r; in >> r.stageFlags >> r.offset >> r.size; c.push_ranges.push_back(r); }
        else if (k == "set_layout") { SetLayout l; in >> l.set >> l.binding >> l.type >> l.count; c.layouts.push_back(l); }
        else if (k == "groups") in >> c.groups[0] >> c.groups[1] >> c.groups[2];
        else if (k == "buffer") { BufferGroup b; in >> b.index >> b.size >> b.in >> b.out >> b.changed; c.buffers.push_back(b); }
        else if (k == "bind") { Bind b; in >> b.set >> b.binding >> b.element >> b.type >> b.group >> b.offset >> b.range; c.binds.push_back(b); }
        else if (k == "extension") { std::string e; in >> e; c.extensions.push_back(e); }
        else if (k == "features10") in >> c.features10;
        else if (k == "feature") { Feature ft; in >> ft.stype >> ft.file >> ft.size; c.features.push_back(ft); }
        else if (k == "unknown_feature") { uint32_t t; in >> t; c.unknown_features.push_back(t); }
    }
    return c;
}

struct Buffer { VkBuffer buffer = VK_NULL_HANDLE; VkDeviceMemory memory = VK_NULL_HANDLE; };

struct Replay {
    Capture cap;
    VkInstance instance = VK_NULL_HANDLE;
    VkPhysicalDevice physical = VK_NULL_HANDLE;
    VkPhysicalDeviceProperties props{};
    VkPhysicalDeviceMemoryProperties mem{};
    VkDevice device = VK_NULL_HANDLE;
    VkQueue queue = VK_NULL_HANDLE;
    uint32_t family = 0;
    bool executable_properties = false, debug_utils = false, want_counters = false, performance_query = false;
    std::vector<std::string> notes;

    uint32_t memory_type(uint32_t bits, VkMemoryPropertyFlags want) {
        // Host-visible buffers are read back byte by byte: prefer cached memory.
        if (want & VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT)
            for (uint32_t i = 0; i < mem.memoryTypeCount; i++)
                if ((bits & (1u << i)) && (mem.memoryTypes[i].propertyFlags & (want | VK_MEMORY_PROPERTY_HOST_CACHED_BIT)) == (want | VK_MEMORY_PROPERTY_HOST_CACHED_BIT)) return i;
        for (uint32_t i = 0; i < mem.memoryTypeCount; i++)
            if ((bits & (1u << i)) && (mem.memoryTypes[i].propertyFlags & want) == want) return i;
        die("no suitable memory type");
    }
    // A type with exactly these properties: a placement experiment must not drift
    // into cached or device-local memory the way the preference above does.
    uint32_t memory_type_exact(uint32_t bits, VkMemoryPropertyFlags want) {
        for (uint32_t i = 0; i < mem.memoryTypeCount; i++)
            if ((bits & (1u << i)) && mem.memoryTypes[i].propertyFlags == want) return i;
        die("no memory type with exactly the requested properties");
    }

    Buffer buffer(VkDeviceSize size, VkBufferUsageFlags usage, VkMemoryPropertyFlags props_, bool exact = false) {
        Buffer b;
        VkBufferCreateInfo info{VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO};
        info.size = std::max<VkDeviceSize>(size, 4);
        info.usage = usage;
        check(vkCreateBuffer(device, &info, nullptr, &b.buffer), "vkCreateBuffer");
        VkMemoryRequirements req;
        vkGetBufferMemoryRequirements(device, b.buffer, &req);
        VkMemoryAllocateInfo alloc{VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO};
        alloc.allocationSize = req.size;
        alloc.memoryTypeIndex = exact ? memory_type_exact(req.memoryTypeBits, props_) : memory_type(req.memoryTypeBits, props_);
        check(vkAllocateMemory(device, &alloc, nullptr, &b.memory), "vkAllocateMemory");
        check(vkBindBufferMemory(device, b.buffer, b.memory, 0), "vkBindBufferMemory");
        return b;
    }

    void create_device() {
        VkApplicationInfo app{VK_STRUCTURE_TYPE_APPLICATION_INFO};
        app.pApplicationName = "rocprof-studio-vkreplay";
        app.apiVersion = std::max<uint32_t>(cap.api, VK_API_VERSION_1_2);
        uint32_t count = 0;
        vkEnumerateInstanceExtensionProperties(nullptr, &count, nullptr);
        std::vector<VkExtensionProperties> iext(count);
        vkEnumerateInstanceExtensionProperties(nullptr, &count, iext.data());
        std::vector<const char *> instance_ext;
        for (auto &e : iext)
            if (!strcmp(e.extensionName, VK_EXT_DEBUG_UTILS_EXTENSION_NAME)) { instance_ext.push_back(VK_EXT_DEBUG_UTILS_EXTENSION_NAME); debug_utils = true; }
        VkInstanceCreateInfo ii{VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO};
        ii.pApplicationInfo = &app;
        ii.enabledExtensionCount = (uint32_t)instance_ext.size();
        ii.ppEnabledExtensionNames = instance_ext.data();
        check(vkCreateInstance(&ii, nullptr, &instance), "vkCreateInstance");
        vkEnumeratePhysicalDevices(instance, &count, nullptr);
        std::vector<VkPhysicalDevice> devs(count);
        vkEnumeratePhysicalDevices(instance, &count, devs.data());
        for (auto d : devs) {
            VkPhysicalDeviceProperties p;
            vkGetPhysicalDeviceProperties(d, &p);
            if (p.vendorID == cap.vendor && p.deviceID == cap.device) { physical = d; props = p; break; }
        }
        if (!physical) die("the captured GPU (vendor " + std::to_string(cap.vendor) + ", device " + std::to_string(cap.device) + ") is not available");
        vkGetPhysicalDeviceMemoryProperties(physical, &mem);
        vkGetPhysicalDeviceQueueFamilyProperties(physical, &count, nullptr);
        std::vector<VkQueueFamilyProperties> families(count);
        vkGetPhysicalDeviceQueueFamilyProperties(physical, &count, families.data());
        family = UINT32_MAX;
        for (uint32_t i = 0; i < count; i++)
            if ((families[i].queueFlags & VK_QUEUE_COMPUTE_BIT) && families[i].timestampValidBits) { family = i; break; }
        if (family == UINT32_MAX) die("no compute queue with timestamps");

        vkEnumerateDeviceExtensionProperties(physical, nullptr, &count, nullptr);
        std::vector<VkExtensionProperties> dext(count);
        vkEnumerateDeviceExtensionProperties(physical, nullptr, &count, dext.data());
        auto supported = [&](const std::string &name) {
            return std::any_of(dext.begin(), dext.end(), [&](auto &e) { return name == e.extensionName; });
        };
        std::vector<std::string> names;
        for (auto &e : cap.extensions)
            if (supported(e)) names.push_back(e);
            else notes.push_back("captured extension " + e + " is not supported by this driver");
        bool exec_listed = std::find(names.begin(), names.end(), VK_KHR_PIPELINE_EXECUTABLE_PROPERTIES_EXTENSION_NAME) != names.end();
        if (!exec_listed && supported(VK_KHR_PIPELINE_EXECUTABLE_PROPERTIES_EXTENSION_NAME)) names.push_back(VK_KHR_PIPELINE_EXECUTABLE_PROPERTIES_EXTENSION_NAME);
        executable_properties = std::find(names.begin(), names.end(), VK_KHR_PIPELINE_EXECUTABLE_PROPERTIES_EXTENSION_NAME) != names.end();
        if (want_counters) {
            bool listed = std::find(names.begin(), names.end(), VK_KHR_PERFORMANCE_QUERY_EXTENSION_NAME) != names.end();
            if (!listed && supported(VK_KHR_PERFORMANCE_QUERY_EXTENSION_NAME)) names.push_back(VK_KHR_PERFORMANCE_QUERY_EXTENSION_NAME);
            performance_query = std::find(names.begin(), names.end(), VK_KHR_PERFORMANCE_QUERY_EXTENSION_NAME) != names.end();
            if (!performance_query) notes.push_back("performance counters were requested, but the driver does not support VK_KHR_performance_query");
        }
        std::vector<const char *> ext;
        for (auto &n : names) ext.push_back(n.c_str());

        // Rebuild the application's feature chain from the captured structure bodies.
        std::vector<std::vector<uint8_t>> chain;
        bool exec_feature = false, perf_feature = false;
        for (auto &ft : cap.features) {
            auto body = slurp(cap.dir + "/" + ft.file);
            std::vector<uint8_t> s(sizeof(VkBaseOutStructure) + body.size());
            memcpy(s.data() + sizeof(VkBaseOutStructure), body.data(), body.size());
            ((VkBaseOutStructure *)s.data())->sType = (VkStructureType)ft.stype;
            if (ft.stype == VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_PIPELINE_EXECUTABLE_PROPERTIES_FEATURES_KHR) {
                exec_feature = true;
                ((VkPhysicalDevicePipelineExecutablePropertiesFeaturesKHR *)s.data())->pipelineExecutableInfo = executable_properties;
            }
            if (ft.stype == VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_PERFORMANCE_QUERY_FEATURES_KHR) {
                perf_feature = true;
                if (performance_query) ((VkPhysicalDevicePerformanceQueryFeaturesKHR *)s.data())->performanceCounterQueryPools = VK_TRUE;
            }
            chain.push_back(std::move(s));
        }
        if (performance_query && !perf_feature) {
            std::vector<uint8_t> s(sizeof(VkPhysicalDevicePerformanceQueryFeaturesKHR));
            auto *f = (VkPhysicalDevicePerformanceQueryFeaturesKHR *)s.data();
            f->sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_PERFORMANCE_QUERY_FEATURES_KHR;
            f->performanceCounterQueryPools = VK_TRUE;
            chain.push_back(std::move(s));
        }
        if (executable_properties && !exec_feature) {
            std::vector<uint8_t> s(sizeof(VkPhysicalDevicePipelineExecutablePropertiesFeaturesKHR));
            auto *f = (VkPhysicalDevicePipelineExecutablePropertiesFeaturesKHR *)s.data();
            f->sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_PIPELINE_EXECUTABLE_PROPERTIES_FEATURES_KHR;
            f->pipelineExecutableInfo = VK_TRUE;
            chain.push_back(std::move(s));
        }
        for (auto t : cap.unknown_features) notes.push_back("captured device enabled feature structure " + std::to_string(t) + ", which the replay cannot reproduce");
        VkPhysicalDeviceFeatures2 f2{VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_FEATURES_2};
        if (!cap.features10.empty()) {
            auto body = slurp(cap.dir + "/" + cap.features10);
            memcpy(&f2.features, body.data(), std::min(body.size(), sizeof(f2.features)));
        }
        void *next = nullptr;
        for (auto it = chain.rbegin(); it != chain.rend(); ++it) {
            ((VkBaseOutStructure *)it->data())->pNext = (VkBaseOutStructure *)next;
            next = it->data();
        }
        f2.pNext = next;
        float priority = 1.0f;
        VkDeviceQueueCreateInfo qi{VK_STRUCTURE_TYPE_DEVICE_QUEUE_CREATE_INFO};
        qi.queueFamilyIndex = family;
        qi.queueCount = 1;
        qi.pQueuePriorities = &priority;
        VkDeviceCreateInfo di{VK_STRUCTURE_TYPE_DEVICE_CREATE_INFO};
        di.pNext = &f2;
        di.queueCreateInfoCount = 1;
        di.pQueueCreateInfos = &qi;
        di.enabledExtensionCount = (uint32_t)ext.size();
        di.ppEnabledExtensionNames = ext.data();
        check(vkCreateDevice(physical, &di, nullptr, &device), "vkCreateDevice");
        vkGetDeviceQueue(device, family, 0, &queue);
    }

    VkCommandPool pool = VK_NULL_HANDLE;
    VkCommandBuffer begin() {
        VkCommandBufferAllocateInfo ai{VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO};
        ai.commandPool = pool;
        ai.level = VK_COMMAND_BUFFER_LEVEL_PRIMARY;
        ai.commandBufferCount = 1;
        VkCommandBuffer cb;
        check(vkAllocateCommandBuffers(device, &ai, &cb), "vkAllocateCommandBuffers");
        VkCommandBufferBeginInfo bi{VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO};
        bi.flags = VK_COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT;
        check(vkBeginCommandBuffer(cb, &bi), "vkBeginCommandBuffer");
        return cb;
    }
    void submit(VkCommandBuffer cb) {
        check(vkEndCommandBuffer(cb), "vkEndCommandBuffer");
        VkSubmitInfo si{VK_STRUCTURE_TYPE_SUBMIT_INFO};
        si.commandBufferCount = 1;
        si.pCommandBuffers = &cb;
        check(vkQueueSubmit(queue, 1, &si, VK_NULL_HANDLE), "vkQueueSubmit");
        check(vkQueueWaitIdle(queue), "vkQueueWaitIdle");
        vkFreeCommandBuffers(device, pool, 1, &cb);
    }
    void barrier(VkCommandBuffer cb, VkPipelineStageFlags src, VkAccessFlags sa, VkPipelineStageFlags dst, VkAccessFlags da) {
        VkMemoryBarrier b{VK_STRUCTURE_TYPE_MEMORY_BARRIER};
        b.srcAccessMask = sa;
        b.dstAccessMask = da;
        vkCmdPipelineBarrier(cb, src, dst, 0, 1, &b, 0, nullptr, 0, nullptr);
    }

    int run(int argc, char **argv) {
        std::string spirv_path, isa_path, dump_dir, mode = "stateless", memory = "device";
        int iterations = 20;
        uint64_t flush_mb = 0;
        bool validate = true;
        cap = load(argv[1]);
        spirv_path = cap.dir + "/" + cap.spirv;
        for (int i = 2; i < argc; i++) {
            std::string a = argv[i];
            auto value = [&]() -> std::string { if (i + 1 >= argc) die("missing value for " + a); return argv[++i]; };
            if (a == "--spirv") spirv_path = value();
            else if (a == "--iterations") iterations = std::stoi(value());
            else if (a == "--no-validate") validate = false;
            else if (a == "--isa") isa_path = value();
            else if (a == "--dump-outputs") dump_dir = value();
            else if (a == "--mode") mode = value();
            else if (a == "--memory") memory = value();
            else if (a == "--flush-mb") flush_mb = std::stoull(value());
            else if (a == "--counters") want_counters = true;
            else die("unknown argument " + a);
        }
        if (iterations < 1 || iterations > 100000) die("iterations must be between 1 and 100000");
        if (mode != "stateless" && mode != "sustained") die("--mode must be stateless or sustained");
        if (memory != "device" && memory != "app" && memory != "gtt" && memory != "auto") die("--memory must be device, app, gtt or auto");
        create_device();
        // Where the dispatch's buffers live. "device" is device-local memory, which a
        // small replay usually gets in VRAM; "app" is what ggml asks for on UMA
        // (device-local, host-visible, coherent); "gtt" is host-visible,
        // non-device-local memory, i.e. system pages mapped through the GART; "auto"
        // is gtt on an integrated GPU, where large application buffers usually end
        // up, and device-local memory on a discrete one.
        if (memory == "auto") memory = props.deviceType == VK_PHYSICAL_DEVICE_TYPE_INTEGRATED_GPU ? "gtt" : "device";
        VkMemoryPropertyFlags work_memory = VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT;
        bool work_exact = false;
        if (memory == "app") { work_memory |= VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT | VK_MEMORY_PROPERTY_HOST_COHERENT_BIT; work_exact = true; }
        else if (memory == "gtt") { work_memory = VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT | VK_MEMORY_PROPERTY_HOST_COHERENT_BIT; work_exact = true; }

        VkCommandPoolCreateInfo pi{VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO};
        pi.queueFamilyIndex = family;
        pi.flags = VK_COMMAND_POOL_CREATE_RESET_COMMAND_BUFFER_BIT;
        check(vkCreateCommandPool(device, &pi, nullptr, &pool), "vkCreateCommandPool");

        // One replay buffer per captured application buffer, so aliasing bindings
        // still alias; a pristine copy of the inputs restores them per iteration.
        const VkBufferUsageFlags work_usage = VK_BUFFER_USAGE_STORAGE_BUFFER_BIT | VK_BUFFER_USAGE_UNIFORM_BUFFER_BIT | VK_BUFFER_USAGE_TRANSFER_SRC_BIT | VK_BUFFER_USAGE_TRANSFER_DST_BIT;
        std::vector<Buffer> work, pristine;
        for (auto &g : cap.buffers) {
            work.push_back(buffer(g.size, work_usage, work_memory, work_exact));
            pristine.push_back(buffer(g.size, VK_BUFFER_USAGE_TRANSFER_SRC_BIT | VK_BUFFER_USAGE_TRANSFER_DST_BIT, VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT));
            auto data = slurp(cap.dir + "/" + g.in);
            if (data.size() != g.size) die("input " + g.in + " has the wrong size");
            Buffer up = buffer(g.size, VK_BUFFER_USAGE_TRANSFER_SRC_BIT, VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT | VK_MEMORY_PROPERTY_HOST_COHERENT_BIT);
            void *p;
            check(vkMapMemory(device, up.memory, 0, g.size, 0, &p), "vkMapMemory");
            memcpy(p, data.data(), g.size);
            vkUnmapMemory(device, up.memory);
            VkCommandBuffer cb = begin();
            VkBufferCopy region{0, 0, g.size};
            vkCmdCopyBuffer(cb, up.buffer, pristine.back().buffer, 1, &region);
            submit(cb);
            vkDestroyBuffer(device, up.buffer, nullptr);
            vkFreeMemory(device, up.memory, nullptr);
        }

        // Descriptor layouts, sets and the pipeline, exactly as captured.
        std::map<uint32_t, std::vector<VkDescriptorSetLayoutBinding>> per_set;
        uint32_t max_set = 0;
        for (auto &l : cap.layouts) {
            per_set[l.set].push_back({l.binding, (VkDescriptorType)l.type, l.count, VK_SHADER_STAGE_COMPUTE_BIT, nullptr});
            max_set = std::max(max_set, l.set);
        }
        std::vector<VkDescriptorSetLayout> set_layouts(per_set.empty() ? 0 : max_set + 1);
        std::map<VkDescriptorType, uint32_t> type_counts;
        for (uint32_t s = 0; s < set_layouts.size(); s++) {
            VkDescriptorSetLayoutCreateInfo li{VK_STRUCTURE_TYPE_DESCRIPTOR_SET_LAYOUT_CREATE_INFO};
            li.bindingCount = (uint32_t)per_set[s].size();
            li.pBindings = per_set[s].data();
            check(vkCreateDescriptorSetLayout(device, &li, nullptr, &set_layouts[s]), "vkCreateDescriptorSetLayout");
            for (auto &b : per_set[s]) type_counts[b.descriptorType] += b.descriptorCount;
        }
        VkPipelineLayoutCreateInfo pli{VK_STRUCTURE_TYPE_PIPELINE_LAYOUT_CREATE_INFO};
        pli.setLayoutCount = (uint32_t)set_layouts.size();
        pli.pSetLayouts = set_layouts.data();
        pli.pushConstantRangeCount = (uint32_t)cap.push_ranges.size();
        pli.pPushConstantRanges = cap.push_ranges.data();
        VkPipelineLayout layout;
        check(vkCreatePipelineLayout(device, &pli, nullptr, &layout), "vkCreatePipelineLayout");
        std::vector<VkDescriptorPoolSize> sizes;
        for (auto &[t, n] : type_counts) sizes.push_back({t, n});
        VkDescriptorPoolCreateInfo dpi{VK_STRUCTURE_TYPE_DESCRIPTOR_POOL_CREATE_INFO};
        dpi.maxSets = std::max<uint32_t>(1, (uint32_t)set_layouts.size());
        dpi.poolSizeCount = (uint32_t)sizes.size();
        dpi.pPoolSizes = sizes.data();
        VkDescriptorPool dpool;
        check(vkCreateDescriptorPool(device, &dpi, nullptr, &dpool), "vkCreateDescriptorPool");
        std::vector<VkDescriptorSet> sets(set_layouts.size());
        if (!sets.empty()) {
            VkDescriptorSetAllocateInfo dai{VK_STRUCTURE_TYPE_DESCRIPTOR_SET_ALLOCATE_INFO};
            dai.descriptorPool = dpool;
            dai.descriptorSetCount = (uint32_t)sets.size();
            dai.pSetLayouts = set_layouts.data();
            check(vkAllocateDescriptorSets(device, &dai, sets.data()), "vkAllocateDescriptorSets");
        }
        std::vector<VkDescriptorBufferInfo> infos(cap.binds.size());
        std::vector<VkWriteDescriptorSet> writes;
        for (size_t i = 0; i < cap.binds.size(); i++) {
            auto &b = cap.binds[i];
            infos[i] = {work[b.group].buffer, b.offset, b.range};
            VkWriteDescriptorSet w{VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET};
            w.dstSet = sets[b.set];
            w.dstBinding = b.binding;
            w.dstArrayElement = b.element;
            w.descriptorCount = 1;
            w.descriptorType = (VkDescriptorType)b.type;
            w.pBufferInfo = &infos[i];
            writes.push_back(w);
        }
        vkUpdateDescriptorSets(device, (uint32_t)writes.size(), writes.data(), 0, nullptr);

        auto code = slurp(spirv_path);
        if (code.size() % 4) die("SPIR-V size is not a multiple of 4");
        VkShaderModuleCreateInfo smi{VK_STRUCTURE_TYPE_SHADER_MODULE_CREATE_INFO};
        smi.codeSize = code.size();
        smi.pCode = (const uint32_t *)code.data();
        VkShaderModule module;
        check(vkCreateShaderModule(device, &smi, nullptr, &module), "vkCreateShaderModule");
        auto spec_data = cap.spec_file.empty() ? std::vector<uint8_t>() : slurp(cap.dir + "/" + cap.spec_file);
        std::vector<VkSpecializationMapEntry> entries;
        for (auto &s : cap.spec) entries.push_back({s.id, s.offset, s.size});
        VkSpecializationInfo spec{(uint32_t)entries.size(), entries.data(), spec_data.size(), spec_data.data()};
        VkPipelineShaderStageRequiredSubgroupSizeCreateInfo subgroup{VK_STRUCTURE_TYPE_PIPELINE_SHADER_STAGE_REQUIRED_SUBGROUP_SIZE_CREATE_INFO};
        subgroup.requiredSubgroupSize = cap.subgroup;
        VkComputePipelineCreateInfo ci{VK_STRUCTURE_TYPE_COMPUTE_PIPELINE_CREATE_INFO};
        ci.stage.sType = VK_STRUCTURE_TYPE_PIPELINE_SHADER_STAGE_CREATE_INFO;
        ci.stage.pNext = cap.subgroup ? &subgroup : nullptr;
        ci.stage.flags = cap.stage_flags;
        ci.stage.stage = VK_SHADER_STAGE_COMPUTE_BIT;
        ci.stage.module = module;
        ci.stage.pName = cap.entry.c_str();
        ci.stage.pSpecializationInfo = entries.empty() ? nullptr : &spec;
        ci.layout = layout;
        ci.flags = cap.pipeline_flags & ~(VK_PIPELINE_CREATE_DERIVATIVE_BIT | VK_PIPELINE_CREATE_ALLOW_DERIVATIVES_BIT);
        if (executable_properties) ci.flags |= VK_PIPELINE_CREATE_CAPTURE_STATISTICS_BIT_KHR | VK_PIPELINE_CREATE_CAPTURE_INTERNAL_REPRESENTATIONS_BIT_KHR;
        VkPipeline pipeline;
        check(vkCreateComputePipelines(device, VK_NULL_HANDLE, 1, &ci, nullptr, &pipeline), "vkCreateComputePipelines");
        if (debug_utils && !cap.name.empty()) {
            auto name_fn = (PFN_vkSetDebugUtilsObjectNameEXT)vkGetInstanceProcAddr(instance, "vkSetDebugUtilsObjectNameEXT");
            VkDebugUtilsObjectNameInfoEXT ni{VK_STRUCTURE_TYPE_DEBUG_UTILS_OBJECT_NAME_INFO_EXT};
            ni.objectType = VK_OBJECT_TYPE_PIPELINE;
            ni.objectHandle = (uint64_t)pipeline;
            ni.pObjectName = cap.name.c_str();
            if (name_fn) name_fn(device, &ni);
        }
        auto push = cap.push_file.empty() ? std::vector<uint8_t>() : slurp(cap.dir + "/" + cap.push_file);
        auto begin_label = debug_utils ? (PFN_vkCmdBeginDebugUtilsLabelEXT)vkGetInstanceProcAddr(instance, "vkCmdBeginDebugUtilsLabelEXT") : nullptr;
        auto end_label = debug_utils ? (PFN_vkCmdEndDebugUtilsLabelEXT)vkGetInstanceProcAddr(instance, "vkCmdEndDebugUtilsLabelEXT") : nullptr;

        auto restore = [&](VkCommandBuffer cb) {
            barrier(cb, VK_PIPELINE_STAGE_COMPUTE_SHADER_BIT | VK_PIPELINE_STAGE_TRANSFER_BIT, VK_ACCESS_SHADER_WRITE_BIT | VK_ACCESS_TRANSFER_WRITE_BIT,
                    VK_PIPELINE_STAGE_TRANSFER_BIT, VK_ACCESS_TRANSFER_WRITE_BIT | VK_ACCESS_TRANSFER_READ_BIT);
            for (size_t g = 0; g < cap.buffers.size(); g++) {
                VkBufferCopy region{0, 0, cap.buffers[g].size};
                vkCmdCopyBuffer(cb, pristine[g].buffer, work[g].buffer, 1, &region);
            }
            barrier(cb, VK_PIPELINE_STAGE_TRANSFER_BIT, VK_ACCESS_TRANSFER_WRITE_BIT, VK_PIPELINE_STAGE_COMPUTE_SHADER_BIT, VK_ACCESS_SHADER_READ_BIT | VK_ACCESS_SHADER_WRITE_BIT);
        };
        auto dispatch = [&](VkCommandBuffer cb) {
            vkCmdBindPipeline(cb, VK_PIPELINE_BIND_POINT_COMPUTE, pipeline);
            if (!sets.empty()) vkCmdBindDescriptorSets(cb, VK_PIPELINE_BIND_POINT_COMPUTE, layout, 0, (uint32_t)sets.size(), sets.data(), 0, nullptr);
            for (auto &r : cap.push_ranges)
                if (r.offset + r.size <= push.size()) vkCmdPushConstants(cb, layout, r.stageFlags, r.offset, r.size, push.data() + r.offset);
            if (begin_label) {
                std::string label = cap.name + " (" + std::to_string(cap.groups[0]) + "," + std::to_string(cap.groups[1]) + "," + std::to_string(cap.groups[2]) + ")";
                VkDebugUtilsLabelEXT l{VK_STRUCTURE_TYPE_DEBUG_UTILS_LABEL_EXT};
                l.pLabelName = label.c_str();
                begin_label(cb, &l);
            }
            vkCmdDispatch(cb, cap.groups[0], cap.groups[1], cap.groups[2]);
            if (end_label) end_label(cb);
        };

        // Timing.
        VkQueryPoolCreateInfo qpi{VK_STRUCTURE_TYPE_QUERY_POOL_CREATE_INFO};
        qpi.queryType = VK_QUERY_TYPE_TIMESTAMP;
        qpi.queryCount = 2 * (uint32_t)iterations;
        VkQueryPool queries;
        check(vkCreateQueryPool(device, &qpi, nullptr, &queries), "vkCreateQueryPool");
        // Stateless: restore every input before each timed dispatch. Sustained: restore
        // once and dispatch back to back, as an application keeps the GPU busy; only
        // valid for timing a dispatch that does not read what it writes. Either can
        // copy a scratch buffer before each dispatch to evict the caches.
        Buffer flush_src, flush_dst;
        if (flush_mb) {
            flush_src = buffer(flush_mb << 20, VK_BUFFER_USAGE_TRANSFER_SRC_BIT, VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT);
            flush_dst = buffer(flush_mb << 20, VK_BUFFER_USAGE_TRANSFER_DST_BIT, VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT);
        }
        auto flush = [&](VkCommandBuffer cb) {
            if (!flush_mb) return;
            barrier(cb, VK_PIPELINE_STAGE_COMPUTE_SHADER_BIT | VK_PIPELINE_STAGE_TRANSFER_BIT, VK_ACCESS_SHADER_WRITE_BIT | VK_ACCESS_TRANSFER_WRITE_BIT,
                    VK_PIPELINE_STAGE_TRANSFER_BIT, VK_ACCESS_TRANSFER_WRITE_BIT);
            VkBufferCopy region{0, 0, flush_mb << 20};
            vkCmdCopyBuffer(cb, flush_src.buffer, flush_dst.buffer, 1, &region);
            barrier(cb, VK_PIPELINE_STAGE_TRANSFER_BIT, VK_ACCESS_TRANSFER_WRITE_BIT, VK_PIPELINE_STAGE_COMPUTE_SHADER_BIT, VK_ACCESS_SHADER_READ_BIT | VK_ACCESS_SHADER_WRITE_BIT);
        };
        VkCommandBuffer cb = begin();
        vkCmdResetQueryPool(cb, queries, 0, qpi.queryCount);
        if (mode == "sustained") restore(cb);
        for (int i = 0; i < iterations; i++) {
            if (mode == "stateless") restore(cb);
            else barrier(cb, VK_PIPELINE_STAGE_COMPUTE_SHADER_BIT, VK_ACCESS_SHADER_WRITE_BIT, VK_PIPELINE_STAGE_COMPUTE_SHADER_BIT, VK_ACCESS_SHADER_READ_BIT | VK_ACCESS_SHADER_WRITE_BIT);
            flush(cb);
            vkCmdWriteTimestamp(cb, VK_PIPELINE_STAGE_BOTTOM_OF_PIPE_BIT, queries, 2 * i);
            dispatch(cb);
            vkCmdWriteTimestamp(cb, VK_PIPELINE_STAGE_BOTTOM_OF_PIPE_BIT, queries, 2 * i + 1);
        }
        submit(cb);
        if (mode == "sustained") notes.push_back("sustained mode: inputs were restored once, so timings are valid only if the dispatch does not read what it writes");

        // Driver performance counters, under the same conditions but in separate
        // dispatches, since a counter query drains the GPU around its command.
        // Timestamps bracket each one too, so cycles over time give the clock the
        // dispatch actually ran at.
        std::string counters = "null";
        if (performance_query) {
            auto enum_counters = (PFN_vkEnumeratePhysicalDeviceQueueFamilyPerformanceQueryCountersKHR)vkGetInstanceProcAddr(instance, "vkEnumeratePhysicalDeviceQueueFamilyPerformanceQueryCountersKHR");
            auto get_passes = (PFN_vkGetPhysicalDeviceQueueFamilyPerformanceQueryPassesKHR)vkGetInstanceProcAddr(instance, "vkGetPhysicalDeviceQueueFamilyPerformanceQueryPassesKHR");
            auto acquire = (PFN_vkAcquireProfilingLockKHR)vkGetDeviceProcAddr(device, "vkAcquireProfilingLockKHR");
            auto release = (PFN_vkReleaseProfilingLockKHR)vkGetDeviceProcAddr(device, "vkReleaseProfilingLockKHR");
            uint32_t cn = 0;
            enum_counters(physical, family, &cn, nullptr, nullptr);
            std::vector<VkPerformanceCounterKHR> kinds(cn, {VK_STRUCTURE_TYPE_PERFORMANCE_COUNTER_KHR});
            std::vector<VkPerformanceCounterDescriptionKHR> descs(cn, {VK_STRUCTURE_TYPE_PERFORMANCE_COUNTER_DESCRIPTION_KHR});
            enum_counters(physical, family, &cn, kinds.data(), descs.data());
            std::vector<uint32_t> indices;
            for (uint32_t i = 0; i < cn; i++)
                if (kinds[i].scope == VK_PERFORMANCE_COUNTER_SCOPE_COMMAND_KHR) indices.push_back(i);
            VkQueryPoolPerformanceCreateInfoKHR pci{VK_STRUCTURE_TYPE_QUERY_POOL_PERFORMANCE_CREATE_INFO_KHR};
            pci.queueFamilyIndex = family;
            pci.counterIndexCount = (uint32_t)indices.size();
            pci.pCounterIndices = indices.data();
            uint32_t passes = 0;
            get_passes(physical, &pci, &passes);
            const uint32_t n = (uint32_t)std::min(iterations, 20);
            VkQueryPoolCreateInfo pqi{VK_STRUCTURE_TYPE_QUERY_POOL_CREATE_INFO};
            pqi.pNext = &pci;
            pqi.queryType = VK_QUERY_TYPE_PERFORMANCE_QUERY_KHR;
            pqi.queryCount = n;
            VkQueryPool pool_perf, pool_time;
            check(vkCreateQueryPool(device, &pqi, nullptr, &pool_perf), "vkCreateQueryPool (performance)");
            VkQueryPoolCreateInfo tqi{VK_STRUCTURE_TYPE_QUERY_POOL_CREATE_INFO};
            tqi.queryType = VK_QUERY_TYPE_TIMESTAMP;
            tqi.queryCount = 2 * n;
            check(vkCreateQueryPool(device, &tqi, nullptr, &pool_time), "vkCreateQueryPool (timestamps)");
            VkAcquireProfilingLockInfoKHR lock{VK_STRUCTURE_TYPE_ACQUIRE_PROFILING_LOCK_INFO_KHR};
            lock.timeout = UINT64_MAX;
            check(acquire(device, &lock), "vkAcquireProfilingLockKHR");
            // A performance query cannot be reset in the command buffer that begins it.
            VkCommandBuffer reset = begin();
            vkCmdResetQueryPool(reset, pool_perf, 0, n);
            submit(reset);
            VkCommandBufferAllocateInfo ai{VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO};
            ai.commandPool = pool;
            ai.level = VK_COMMAND_BUFFER_LEVEL_PRIMARY;
            ai.commandBufferCount = 1;
            VkCommandBuffer mcb;
            check(vkAllocateCommandBuffers(device, &ai, &mcb), "vkAllocateCommandBuffers");
            VkCommandBufferBeginInfo bi{VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO};
            check(vkBeginCommandBuffer(mcb, &bi), "vkBeginCommandBuffer");
            vkCmdResetQueryPool(mcb, pool_time, 0, 2 * n);
            if (mode == "sustained") restore(mcb);
            for (uint32_t i = 0; i < n; i++) {
                if (mode == "stateless") restore(mcb);
                else barrier(mcb, VK_PIPELINE_STAGE_COMPUTE_SHADER_BIT, VK_ACCESS_SHADER_WRITE_BIT, VK_PIPELINE_STAGE_COMPUTE_SHADER_BIT, VK_ACCESS_SHADER_READ_BIT | VK_ACCESS_SHADER_WRITE_BIT);
                flush(mcb);
                vkCmdBeginQuery(mcb, pool_perf, i, 0);
                vkCmdWriteTimestamp(mcb, VK_PIPELINE_STAGE_BOTTOM_OF_PIPE_BIT, pool_time, 2 * i);
                dispatch(mcb);
                vkCmdWriteTimestamp(mcb, VK_PIPELINE_STAGE_BOTTOM_OF_PIPE_BIT, pool_time, 2 * i + 1);
                vkCmdEndQuery(mcb, pool_perf, i);
            }
            check(vkEndCommandBuffer(mcb), "vkEndCommandBuffer");
            std::vector<double> pass_ns;
            for (uint32_t p = 0; p < passes; p++) {
                VkPerformanceQuerySubmitInfoKHR psi{VK_STRUCTURE_TYPE_PERFORMANCE_QUERY_SUBMIT_INFO_KHR};
                psi.counterPassIndex = p;
                VkSubmitInfo si{VK_STRUCTURE_TYPE_SUBMIT_INFO};
                si.pNext = &psi;
                si.commandBufferCount = 1;
                si.pCommandBuffers = &mcb;
                check(vkQueueSubmit(queue, 1, &si, VK_NULL_HANDLE), "vkQueueSubmit (counters)");
                check(vkQueueWaitIdle(queue), "vkQueueWaitIdle");
                std::vector<uint64_t> st(2 * n);
                check(vkGetQueryPoolResults(device, pool_time, 0, 2 * n, st.size() * 8, st.data(), 8, VK_QUERY_RESULT_64_BIT | VK_QUERY_RESULT_WAIT_BIT), "vkGetQueryPoolResults (timestamps)");
                for (uint32_t i = 0; i < n; i++) pass_ns.push_back((st[2 * i + 1] - st[2 * i]) * (double)props.limits.timestampPeriod);
            }
            std::vector<VkPerformanceCounterResultKHR> res(n * indices.size());
            check(vkGetQueryPoolResults(device, pool_perf, 0, n, res.size() * sizeof(res[0]), res.data(), indices.size() * sizeof(res[0]), VK_QUERY_RESULT_WAIT_BIT),
                  "vkGetQueryPoolResults (counters)");
            release(device);
            auto value = [&](const VkPerformanceCounterKHR &k, const VkPerformanceCounterResultKHR &r) -> double {
                switch (k.storage) {
                case VK_PERFORMANCE_COUNTER_STORAGE_INT32_KHR: return r.int32;
                case VK_PERFORMANCE_COUNTER_STORAGE_INT64_KHR: return (double)r.int64;
                case VK_PERFORMANCE_COUNTER_STORAGE_UINT32_KHR: return r.uint32;
                case VK_PERFORMANCE_COUNTER_STORAGE_UINT64_KHR: return (double)r.uint64;
                case VK_PERFORMANCE_COUNTER_STORAGE_FLOAT32_KHR: return r.float32;
                default: return r.float64;
                }
            };
            auto median_of = [](std::vector<double> v) { std::sort(v.begin(), v.end()); return v[v.size() / 2]; };
            std::ostringstream c;
            c.precision(10);
            c << "{\"passes\":" << passes << ",\"dispatches\":" << n << ",\"durationNs\":" << median_of(pass_ns) << ",\"values\":{";
            for (size_t j = 0; j < indices.size(); j++) {
                std::vector<double> v;
                for (uint32_t i = 0; i < n; i++) v.push_back(value(kinds[indices[j]], res[i * indices.size() + j]));
                c << (j ? "," : "") << json_string(descs[indices[j]].name) << ":" << median_of(v);
            }
            c << "}}";
            counters = c.str();
            vkDestroyQueryPool(device, pool_perf, nullptr);
            vkDestroyQueryPool(device, pool_time, nullptr);
        }
        std::vector<uint64_t> stamps(qpi.queryCount);
        check(vkGetQueryPoolResults(device, queries, 0, qpi.queryCount, stamps.size() * 8, stamps.data(), 8, VK_QUERY_RESULT_64_BIT | VK_QUERY_RESULT_WAIT_BIT), "vkGetQueryPoolResults");
        std::vector<double> ns;
        for (int i = 0; i < iterations; i++) ns.push_back((stamps[2 * i + 1] - stamps[2 * i]) * (double)props.limits.timestampPeriod);
        std::vector<double> sorted = ns;
        std::sort(sorted.begin(), sorted.end());
        double median = sorted[sorted.size() / 2], mean = std::accumulate(ns.begin(), ns.end(), 0.0) / ns.size();

        // Validation against the application's own post-dispatch contents: one
        // dispatch for correctness and, for sustained timing, which repeats the
        // dispatch over its own outputs, two back to back.
        auto check_outputs = [&](int dispatches, bool dump, std::string &groups) {
            cb = begin();
            restore(cb);
            for (int d = 0; d < dispatches; d++) {
                if (d) barrier(cb, VK_PIPELINE_STAGE_COMPUTE_SHADER_BIT, VK_ACCESS_SHADER_WRITE_BIT, VK_PIPELINE_STAGE_COMPUTE_SHADER_BIT, VK_ACCESS_SHADER_READ_BIT | VK_ACCESS_SHADER_WRITE_BIT);
                dispatch(cb);
            }
            barrier(cb, VK_PIPELINE_STAGE_COMPUTE_SHADER_BIT, VK_ACCESS_SHADER_WRITE_BIT, VK_PIPELINE_STAGE_TRANSFER_BIT, VK_ACCESS_TRANSFER_READ_BIT);
            std::vector<Buffer> down;
            for (size_t g = 0; g < cap.buffers.size(); g++) {
                down.push_back(buffer(cap.buffers[g].size, VK_BUFFER_USAGE_TRANSFER_DST_BIT, VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT | VK_MEMORY_PROPERTY_HOST_COHERENT_BIT));
                VkBufferCopy region{0, 0, cap.buffers[g].size};
                vkCmdCopyBuffer(cb, work[g].buffer, down.back().buffer, 1, &region);
            }
            barrier(cb, VK_PIPELINE_STAGE_TRANSFER_BIT, VK_ACCESS_TRANSFER_WRITE_BIT, VK_PIPELINE_STAGE_HOST_BIT, VK_ACCESS_HOST_READ_BIT);
            submit(cb);
            std::ostringstream v;
            bool all = true;
            v << "[";
            for (size_t g = 0; g < cap.buffers.size(); g++) {
                auto input = slurp(cap.dir + "/" + cap.buffers[g].in);
                auto expected = expected_output(cap, cap.buffers[g], input);
                void *p;
                check(vkMapMemory(device, down[g].memory, 0, cap.buffers[g].size, 0, &p), "vkMapMemory");
                const uint8_t *got = (const uint8_t *)p;
                uint64_t mismatched = 0, changed = 0, first = UINT64_MAX;
                for (uint64_t i = 0; i < cap.buffers[g].size; i++) {
                    if (got[i] != expected[i]) { mismatched++; if (first == UINT64_MAX) first = i; }
                    if (expected[i] != input[i]) changed++;
                }
                if (dump) {
                    std::ofstream out(dump_dir + "/out_" + std::to_string(g) + ".bin", std::ios::binary);
                    out.write((const char *)got, cap.buffers[g].size);
                }
                vkUnmapMemory(device, down[g].memory);
                vkDestroyBuffer(device, down[g].buffer, nullptr);
                vkFreeMemory(device, down[g].memory, nullptr);
                all = all && mismatched == 0;
                v << (g ? "," : "") << "{\"group\":" << g << ",\"bytes\":" << cap.buffers[g].size << ",\"writtenByApplication\":" << changed
                  << ",\"mismatched\":" << mismatched << ",\"firstMismatch\":" << (first == UINT64_MAX ? std::string("null") : std::to_string(first)) << "}";
            }
            v << "]";
            groups = v.str();
            return all;
        };
        std::string validation = "\"not checked\"";
        if (validate) {
            std::string groups, repeated;
            bool match = check_outputs(1, !dump_dir.empty(), groups);
            validation = std::string("{\"match\":") + (match ? "true" : "false");
            if (mode == "sustained") validation += std::string(",\"sustainedMatch\":") + (check_outputs(2, false, repeated) ? "true" : "false");
            validation += ",\"groups\":" + groups + "}";
        }

        // Compiler statistics and ISA, when the driver exposes them.
        std::string stats = "{}";
        if (executable_properties) {
            auto get_props = (PFN_vkGetPipelineExecutablePropertiesKHR)vkGetDeviceProcAddr(device, "vkGetPipelineExecutablePropertiesKHR");
            auto get_stats = (PFN_vkGetPipelineExecutableStatisticsKHR)vkGetDeviceProcAddr(device, "vkGetPipelineExecutableStatisticsKHR");
            auto get_ir = (PFN_vkGetPipelineExecutableInternalRepresentationsKHR)vkGetDeviceProcAddr(device, "vkGetPipelineExecutableInternalRepresentationsKHR");
            VkPipelineInfoKHR info{VK_STRUCTURE_TYPE_PIPELINE_INFO_KHR};
            info.pipeline = pipeline;
            uint32_t n = 0;
            get_props(device, &info, &n, nullptr);
            if (n) {
                VkPipelineExecutableInfoKHR ei{VK_STRUCTURE_TYPE_PIPELINE_EXECUTABLE_INFO_KHR};
                ei.pipeline = pipeline;
                ei.executableIndex = 0;
                uint32_t sn = 0;
                get_stats(device, &ei, &sn, nullptr);
                std::vector<VkPipelineExecutableStatisticKHR> st(sn, {VK_STRUCTURE_TYPE_PIPELINE_EXECUTABLE_STATISTIC_KHR});
                get_stats(device, &ei, &sn, st.data());
                std::ostringstream o;
                o << "{";
                for (uint32_t i = 0; i < sn; i++) {
                    o << (i ? "," : "") << json_string(st[i].name) << ":";
                    switch (st[i].format) {
                    case VK_PIPELINE_EXECUTABLE_STATISTIC_FORMAT_BOOL32_KHR: o << (st[i].value.b32 ? "true" : "false"); break;
                    case VK_PIPELINE_EXECUTABLE_STATISTIC_FORMAT_INT64_KHR: o << st[i].value.i64; break;
                    case VK_PIPELINE_EXECUTABLE_STATISTIC_FORMAT_UINT64_KHR: o << st[i].value.u64; break;
                    default: o << st[i].value.f64;
                    }
                }
                o << "}";
                stats = o.str();
                if (!isa_path.empty()) {
                    uint32_t rn = 0;
                    get_ir(device, &ei, &rn, nullptr);
                    std::vector<VkPipelineExecutableInternalRepresentationKHR> ir(rn, {VK_STRUCTURE_TYPE_PIPELINE_EXECUTABLE_INTERNAL_REPRESENTATION_KHR});
                    get_ir(device, &ei, &rn, ir.data());
                    std::vector<std::vector<char>> text(rn);
                    for (uint32_t i = 0; i < rn; i++) { text[i].resize(ir[i].dataSize); ir[i].pData = text[i].data(); }
                    get_ir(device, &ei, &rn, ir.data());
                    std::ofstream out(isa_path);
                    for (uint32_t i = 0; i < rn; i++)
                        if (std::string(ir[i].name).find("ssembly") != std::string::npos) out.write(text[i].data(), strnlen(text[i].data(), text[i].size()));
                }
            }
        }

        std::ostringstream o;
        o.precision(10);
        const char *device_type = props.deviceType == VK_PHYSICAL_DEVICE_TYPE_INTEGRATED_GPU ? "integrated"
                                  : props.deviceType == VK_PHYSICAL_DEVICE_TYPE_DISCRETE_GPU ? "discrete" : "other";
        o << "{\"device\":" << json_string(props.deviceName) << ",\"deviceType\":\"" << device_type << "\",\"driverVersion\":" << props.driverVersion
          << ",\"name\":" << json_string(cap.name) << ",\"groups\":[" << cap.groups[0] << "," << cap.groups[1] << "," << cap.groups[2] << "]"
          << ",\"execution\":{\"mode\":" << json_string(mode) << ",\"memory\":" << json_string(memory) << ",\"flushBytes\":" << (flush_mb << 20)
          << "},\"timing\":{\"unit\":\"ns\",\"median\":" << median << ",\"mean\":" << mean
          << ",\"min\":" << sorted.front() << ",\"max\":" << sorted.back() << ",\"samples\":[";
        for (size_t i = 0; i < ns.size(); i++) o << (i ? "," : "") << ns[i];
        o << "]},\"statistics\":" << stats << ",\"counters\":" << counters << ",\"validation\":" << validation << ",\"notes\":[";
        for (size_t i = 0; i < notes.size(); i++) o << (i ? "," : "") << json_string(notes[i]);
        o << "]}\n";
        fputs(o.str().c_str(), stdout);
        return 0;
    }
};

}  // namespace

int main(int argc, char **argv) {
    if (argc < 2) {
        fprintf(stderr, "usage: vkreplay CAPTURE_DIR [--spirv FILE] [--iterations N] [--no-validate] [--isa FILE] [--dump-outputs DIR]\n"
                        "                [--mode stateless|sustained] [--memory device|app|gtt|auto] [--flush-mb N] [--counters]\n");
        return 2;
    }
    try {
        return Replay{}.run(argc, argv);
    } catch (const std::exception &e) {
        fprintf(stderr, "vkreplay: %s\n", e.what());
        return 1;
    }
}
