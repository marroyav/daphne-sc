#include "registry.hpp"

#include <cstdlib>
#include <iostream>
#include <stdexcept>
#include <string>

namespace {

size_t expected(const char *value, const char *name) {
    try {
        return static_cast<size_t>(std::stoull(value));
    } catch(const std::exception &) {
        throw std::runtime_error(std::string("invalid expected ") + name + ": " + value);
    }
}

} // namespace

int main(int argc, char **argv) {
    if(argc != 2 && argc != 5) {
        std::cerr << "usage: pds-registry-check TAG_LIST.csv "
                     "[EXPECTED_PATTERNS EXPECTED_READ_PATTERNS EXPECTED_METHOD_PATTERNS]\n";
        return 2;
    }

    try {
        const auto entries = pds::registry::loadTagList(argv[1]);
        const auto expanded = pds::registry::expand(
            entries, pds::registry::defaultExpansionConfig());
        const auto summary = pds::registry::summarize(entries, expanded);
        std::cout << "patterns=" << summary.patterns
                  << " read_patterns=" << summary.readOnlyPatterns
                  << " method_patterns=" << summary.writeOnlyPatterns
                  << " expanded_read_nodes=" << summary.expandedReadNodes
                  << " expanded_method_nodes=" << summary.expandedMethodNodes << "\n";

        if(argc == 5 &&
           (summary.patterns != expected(argv[2], "pattern count") ||
            summary.readOnlyPatterns != expected(argv[3], "read pattern count") ||
            summary.writeOnlyPatterns != expected(argv[4], "method pattern count"))) {
            std::cerr << "registry contract count mismatch\n";
            return 1;
        }
        return 0;
    } catch(const std::exception &err) {
        std::cerr << "registry check failed: " << err.what() << "\n";
        return 1;
    }
}
