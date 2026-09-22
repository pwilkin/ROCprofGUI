#pragma once
#include <map>
#include <memory>
#include <vector>
#include "data/wavemanager.h"
#include "hidden_latency.h"
struct WaveEntry { std::shared_ptr<WaveInstance> wave; };
struct OtherSimd { int64_t time; int64_t cycles; };
struct DataStore {
    struct WaveCoordinate { struct {int se; int simd;} hwid; };
    std::map<int,std::map<int,int>> wave_hierarchy;
    std::vector<std::pair<WaveCoordinate,WaveEntry>> waves;
    std::map<int,std::vector<OtherSimd>> other_simd_by_se;
    std::map<int,HiddenLatencyAnalysis::HiddenLatency> hidden_latency_by_line;
    bool hidden_latency_analyzed=false;
    template<class F> void forEachWave(F callback){for(auto& [coord,entry]:waves)callback(coord,entry);}
    auto getWave(const WaveEntry& entry){return entry.wave;}
};
