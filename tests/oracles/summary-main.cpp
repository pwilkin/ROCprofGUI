#include "datanode.h"
#include <iomanip>
#include <iostream>
#include <map>
int main() {
    int count;
    std::cin >> count;
    std::map<int, std::map<int, std::vector<CounterData>>> records;
    for (int i = 0; i < count; i++) {
        int bank, se, cu;
        CounterData row;
        std::cin >> bank >> se >> cu >> row.time;
        row.se = se;
        row.cu = cu;
        for (auto& value : row.events) std::cin >> value;
        records[bank][se].push_back(row);
    }
    std::map<int, GPUCounterNode> nodes;
    for (auto& [bank, ses] : records)
        for (auto& [se, rows] : ses) nodes[bank].Insert(se, rows);
    int64_t delta = INT64_MAX;
    for (auto& [bank, node] : nodes) delta = verify_skew(delta, node.getDelta());
    std::cout << "delta " << delta << '\n';
    for (auto& [bank, node] : nodes) {
        node.fillDelta(delta);
        auto rows = node.AccumFromMask(~uint64_t{0}, ~uint64_t{0});
        for (int c = 0; c < 4; c++) {
            double peak = 0;
            for (auto& row : rows) peak = std::max<double>(peak, row.events[c]);
            std::cout << "peak " << bank << ' ' << c << ' ' << std::setprecision(12) << peak << '\n';
        }
    }
}
