#pragma once
#include <map>
#include <memory>
#include <vector>
#include "hidden_latency.h"
struct ASMCodeline {
    struct {struct {HiddenLatencyAnalysis::HiddenLatency hidden;void clearHidden(){hidden={};}} sqtt;} hotspot;
    static inline std::vector<std::shared_ptr<ASMCodeline>> line_vec;
    static inline std::map<int,std::shared_ptr<ASMCodeline>> line_map;
};
