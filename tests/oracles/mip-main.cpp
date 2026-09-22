#include <algorithm>
#include <array>
#include <cstdint>
#include <iostream>
#include <stdexcept>
#include <vector>
#define QWARNING(condition, message, action) do { action; } while(0)
struct Token {
    int64_t clock = 0;
    int slot = 0, type = 0, cycles = 0;
    bool operator<(const Token& other) const {
        if (clock != other.clock) return clock < other.clock;
        if (slot != other.slot) return slot < other.slot;
        return cycles > other.cycles;
    }
};
struct TokenMap : std::vector<Token> {
    void Compile() { std::stable_sort(begin(), end()); }
};
struct WaveInstance {
    static int base;
    static int64_t BaseClock() { return base; }
};
int WaveInstance::base = 1;
struct TokenGroup {
    struct TokenArray {
        Token token{};
        std::array<int,16> cycles{};
        TokenArray& operator+=(const TokenArray& other) {
            token.cycles = std::max<int64_t>(token.cycles, other.token.clock + other.token.cycles - token.clock);
            for (size_t i=0; i<cycles.size(); i++) cycles[i] += other.cycles[i];
            return *this;
        }
        Token finalize(int64_t);
    };
    bool bInitialized = false;
    TokenMap tokens;
    std::array<TokenMap,9> token_mip;
    void SetMipN();
    void SetMipN(const std::vector<TokenArray>&, size_t);
};
#include "token_mip.inc"
int main() {
    int count;
    while(std::cin >> count >> WaveInstance::base) {
        TokenGroup group;
        for(int i=0; i<count; i++) {
            Token token;
            std::cin >> token.clock >> token.cycles >> token.type >> token.slot;
            group.tokens.push_back(token);
        }
        group.tokens.Compile();
        group.SetMipN();
        for (const auto& level: group.token_mip) {
            std::cout << level.size();
            for(const auto& token: level) std::cout << ' ' << token.clock << ' ' << token.cycles << ' ' << token.type << ' ' << token.slot;
            std::cout << '\n';
        }
    }
}
