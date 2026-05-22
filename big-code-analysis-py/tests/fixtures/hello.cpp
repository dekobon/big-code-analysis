#include <string>

std::string hello(const std::string& name) {
    if (name.empty()) {
        return "hello, world";
    }
    return "hello, " + name;
}
