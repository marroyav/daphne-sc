#pragma once

#include <cstddef>
#include <map>
#include <string>
#include <utility>
#include <vector>

namespace pds::registry {

enum class ValueType {
    Boolean,
    Integer,
    Long,
    Double,
    String,
    DateTime,
};

enum class Access {
    ReadOnly,
    WriteOnly,
};

struct Entry {
    std::string subsystem;
    std::string name;
    std::string description;
    std::string nodeIdPattern;
    ValueType valueType = ValueType::String;
    Access access = Access::ReadOnly;
    std::string engineeringUnits;
    std::string implementationStatus;
    std::string controlOwner;
};

struct ExpandedNode {
    Entry entry;
    std::string nodeId;
};

struct ExpansionConfig {
    std::map<std::string, std::vector<std::string>> instances;
    std::vector<std::pair<std::string, std::string>> i2cDevices;
    size_t maximumNodes = 100000;
};

struct Summary {
    size_t patterns = 0;
    size_t readOnlyPatterns = 0;
    size_t writeOnlyPatterns = 0;
    size_t expandedReadNodes = 0;
    size_t expandedMethodNodes = 0;
};

std::vector<Entry> loadTagList(const std::string &path);
ExpansionConfig defaultExpansionConfig();
std::vector<ExpandedNode> expand(const std::vector<Entry> &entries,
                                 const ExpansionConfig &config);
Summary summarize(const std::vector<Entry> &entries,
                  const std::vector<ExpandedNode> &expanded);

std::vector<std::string> parseInstanceList(const std::string &value);
std::vector<std::pair<std::string, std::string>> parseI2cDevices(
    const std::string &value);
std::string stringNodeId(const std::string &nodeIdPattern);
std::string valueTypeName(ValueType type);

} // namespace pds::registry
