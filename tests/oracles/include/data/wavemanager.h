#pragma once
#include <cstdint>
#include <vector>
struct Token { int64_t clock; int64_t cycles; int64_t stall; int type; int code_line; };
struct WaveInstance { int64_t begin; std::vector<Token> tokens; int64_t WaveBegin() const {return begin;} };
