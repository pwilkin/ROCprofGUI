#pragma once
#include <string>
#include <vector>
#define QWARNING(condition, message, action) if (!(condition)) { action; }
struct Config {
    struct Color { std::string name; };
    static const std::vector<Color>& TokenColors() {
        static const std::vector<Color> colors={{"OTHER"},{"SMEM"},{"SALU"},{"VMEM"},{"FLAT"},{"LDS"},{"VALU"},{"BRANCH"},{"NEXT"},{"IMMED"},{"CONTEXT"},{"MESSAGE"},{"BVH"},{"MATRIX"}};
        return colors;
    }
};
