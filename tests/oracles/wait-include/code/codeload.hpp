#pragma once
#include <string>
struct CodeLine { int index; std::string inst; };
struct CodeData { CodeLine* line; };
