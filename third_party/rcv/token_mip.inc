// MIT License
//
// Copyright (c) 2024-2026 Advanced Micro Devices, Inc. All rights reserved.
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.

static int gettype(const std::array<int, 16>& partial_cycles)
{
    int64_t max_cycles = -1;
    int type = 0;

    for (size_t i = 0; i < partial_cycles.size(); i++)
        if (partial_cycles.at(i) >= max_cycles)
        {
            max_cycles = partial_cycles.at(i);
            type = i;
        }

    return type;
}

Token TokenGroup::TokenArray::finalize(int64_t res)
{
    token.type = gettype(cycles);
    token.clock = (token.clock + res / 2) & ~(res - 1);
    token.cycles = (token.cycles + res / 2) & ~(res - 1);
    return token;
}

void TokenGroup::SetMipN(const std::vector<TokenArray>& previous, size_t M)
{
    const int64_t res = (1 << M) * WaveInstance::BaseClock();

    if (M >= token_mip.size()) return;

    if (previous.size() <= 16)
    {
        for (auto current : previous)
        {
            auto token = current.finalize(res);
            if (token.type > 0) token_mip.at(M).emplace_back(token);
        }
        token_mip.at(M).Compile();
        SetMipN(previous, M + 1);
        return;
    }

    std::vector<TokenArray> next{};

    TokenArray current{};
    int64_t current_cycle = current.token.clock = previous.at(0).token.clock;

    for (auto& prev : previous)
    {
        bool prev_end = prev.token.cycles + prev.token.clock - current.token.clock > 2 * res;
        bool this_end = prev_end && current.token.cycles > res && prev.token.type != gettype(current.cycles);
        if (current_cycle + res / 2 <= prev.token.clock || this_end)
        {
            if (current.token.cycles > 0)
            {
                auto finalized = current.finalize(res);
                next.emplace_back(std::move(current));

                if (finalized.type > 0) token_mip.at(M).emplace_back(finalized);
            }

            current = {};
            current_cycle = current.token.clock = prev.token.clock;
        }
        else if (current_cycle != prev.token.clock)
        {
            // add IDLE time
            current.cycles.at(0) += prev.token.clock - current_cycle;
            current.token.cycles += prev.token.clock - current_cycle;
        }

        current += prev;
        current_cycle = std::max(current_cycle, prev.token.clock + prev.token.cycles);
    }

    auto finalized = current.finalize(res);
    if (finalized.type > 0) token_mip.at(M).emplace_back(finalized);
    next.emplace_back(std::move(current));

    token_mip.at(M).Compile();
    SetMipN(next, M + 1);
}

void TokenGroup::SetMipN()
{
    bInitialized = true;
    for (auto& mip : token_mip) mip.clear();

    std::vector<TokenArray> next{};
    next.reserve(tokens.size());

    for (auto& token : tokens) try
        {
            TokenArray newtoken{token, {}};
            newtoken.cycles.at(token.type) = token.cycles;
            next.emplace_back(std::move(newtoken));
        }
        catch (std::out_of_range& e)
        {
            QWARNING(false, "Invalid token type " << token.type, continue);
        }

    SetMipN(next, 0);
}

