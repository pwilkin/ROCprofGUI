#include <algorithm>
#include <array>
#include <cstdint>
#include <iostream>
#include <vector>
struct Token { int64_t clock, cycles; int slot = 0; };
struct WaveInstance {
    std::vector<Token> tokens;
    void appendTokenWithSlotBump(Token&&, std::array<int64_t, 4>&, std::array<int64_t, 4>&);
};
#include "slot_bump.inc"
int main() {
    int count;
    while (std::cin >> count) {
        WaveInstance wave;
        std::array<int64_t, 4> previous{}, last{};
        for (int i = 0; i < count; i++) {
            Token token{};
            std::cin >> token.clock >> token.cycles;
            wave.appendTokenWithSlotBump(std::move(token), previous, last);
        }
        for (auto& token : wave.tokens) std::cout << token.slot << ' ';
        std::cout << '\n';
    }
}
