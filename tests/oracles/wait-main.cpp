#include <iostream>
#include <memory>
#include <cstdint>
#include "data/waitcnt/analysis.h"
int main() {
    int family, count;
    while (std::cin >> family >> count) {
        std::string text;
        std::getline(std::cin, text);
        std::vector<std::unique_ptr<CodeLine>> lines;
        std::vector<CodeData> code;
        TokenMap tokens;
        for (int i=0; i<count; ++i) {
            std::getline(std::cin,text);
            lines.push_back(std::make_unique<CodeLine>(CodeLine{i,text}));
            code.push_back({lines.back().get()});
            tokens.push_back({i});
        }
        auto results = buildWaitcntFromTokens(family,tokens,code);
        std::cout << results.size() << "\n";
        for (const auto& result: results) {
            std::cout << result.line << " " << result.sources.size();
            for (auto source: result.sources) std::cout << " " << source.first;
            std::cout << "\n";
        }
    }
}
