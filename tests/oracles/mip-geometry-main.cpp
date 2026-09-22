#include <iostream>
#include "util/wave_utils.h"
int main() {
    int64_t value;
    int level, base;
    while (std::cin >> value >> level >> base)
        std::cout << WaveUtils::getTokenSize(value, level, base == 1) << ' ' << WaveUtils::slotHeightReduction(level) << '\n';
}
