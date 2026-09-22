#include <iostream>
#include "data/datastore.h"
int main() {
    DataStore store;
    int count;std::cin>>count;
    while(count--){
        int se,simd,size;int64_t begin;std::cin>>se>>simd>>begin>>size;
        auto wave=std::make_shared<WaveInstance>();wave->begin=begin;
        while(size--){Token t;std::cin>>t.clock>>t.cycles>>t.stall>>t.type>>t.code_line;wave->tokens.push_back(t);}
        store.wave_hierarchy[se][simd]=1;store.waves.push_back({{{se,simd}},{wave}});
    }
    std::cin>>count;
    while(count--){int se;OtherSimd other;std::cin>>se>>other.time>>other.cycles;store.other_simd_by_se[se].push_back(other);}
    if(!HiddenLatencyAnalysis::analyze(store))return 2;
    for(auto& [line,cost]:store.hidden_latency_by_line)std::cout<<line<<" "<<cost.idle<<" "<<cost.stall<<" "<<cost.issue<<"\n";
}
