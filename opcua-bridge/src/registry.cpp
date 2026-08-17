#include "registry.hpp"

#include <algorithm>
#include <cctype>
#include <fstream>
#include <set>
#include <sstream>
#include <stdexcept>

namespace pds::registry {
namespace {

constexpr const char *kNamespacePrefix = "nsu=urn:dune:pds:daphne;s=";

std::string trim(const std::string &value) {
    const auto begin = value.find_first_not_of(" \t\r\n");
    if(begin == std::string::npos)
        return "";
    const auto end = value.find_last_not_of(" \t\r\n");
    return value.substr(begin, end - begin + 1);
}

std::vector<std::string> parseCsvRecord(const std::string &line) {
    std::vector<std::string> fields;
    std::string field;
    bool quoted = false;
    for(size_t index = 0; index < line.size(); ++index) {
        const char ch = line[index];
        if(ch == '"') {
            if(quoted && index + 1 < line.size() && line[index + 1] == '"') {
                field.push_back('"');
                ++index;
            } else {
                quoted = !quoted;
            }
        } else if(ch == ',' && !quoted) {
            fields.push_back(field);
            field.clear();
        } else {
            field.push_back(ch);
        }
    }
    if(quoted)
        throw std::runtime_error("unterminated quoted CSV field");
    fields.push_back(field);
    return fields;
}

ValueType parseValueType(const std::string &value) {
    if(value == "Boolean") return ValueType::Boolean;
    if(value == "Integer") return ValueType::Integer;
    if(value == "Long") return ValueType::Long;
    if(value == "Double") return ValueType::Double;
    if(value == "String") return ValueType::String;
    if(value == "DateTime") return ValueType::DateTime;
    throw std::runtime_error("unsupported registry data type: " + value);
}

Access parseAccess(const std::string &value) {
    if(value == "Read-Only") return Access::ReadOnly;
    if(value == "Write-Only") return Access::WriteOnly;
    throw std::runtime_error("unsupported registry access: " + value);
}

std::string implementationStatus(const std::string &notes) {
    constexpr const char *prefix = "STATUS=";
    const auto begin = notes.find(prefix);
    if(begin == std::string::npos)
        return "UNSPECIFIED";
    const auto valueBegin = begin + std::char_traits<char>::length(prefix);
    const auto end = notes.find(" | ", valueBegin);
    return trim(notes.substr(valueBegin, end == std::string::npos
                                             ? std::string::npos
                                             : end - valueBegin));
}

std::vector<std::string> placeholders(const std::string &pattern) {
    std::vector<std::string> result;
    std::set<std::string> seen;
    size_t cursor = 0;
    while((cursor = pattern.find('{', cursor)) != std::string::npos) {
        const auto end = pattern.find('}', cursor + 1);
        if(end == std::string::npos)
            throw std::runtime_error("unterminated registry placeholder: " + pattern);
        const std::string name = pattern.substr(cursor + 1, end - cursor - 1);
        if(name.empty())
            throw std::runtime_error("empty registry placeholder: " + pattern);
        if(seen.insert(name).second)
            result.push_back(name);
        cursor = end + 1;
    }
    return result;
}

void replaceAll(std::string &value, const std::string &needle,
                const std::string &replacement) {
    size_t cursor = 0;
    while((cursor = value.find(needle, cursor)) != std::string::npos) {
        value.replace(cursor, needle.size(), replacement);
        cursor += replacement.size();
    }
}

void expandRecursive(const Entry &entry, const ExpansionConfig &config,
                     const std::vector<std::string> &names, size_t index,
                     std::string nodeId, std::vector<ExpandedNode> &result) {
    if(result.size() >= config.maximumNodes)
        throw std::runtime_error("registry expansion exceeds configured maximum node count");
    if(index == names.size()) {
        if(nodeId.find('{') != std::string::npos)
            throw std::runtime_error("unexpanded registry NodeId: " + nodeId);
        result.push_back(ExpandedNode{entry, std::move(nodeId)});
        return;
    }

    const std::string &name = names[index];
    const auto found = config.instances.find(name);
    if(found == config.instances.end() || found->second.empty())
        throw std::runtime_error("no controlled instances configured for {" + name + "}");
    for(const auto &instance : found->second) {
        std::string expanded = nodeId;
        replaceAll(expanded, "{" + name + "}", instance);
        expandRecursive(entry, config, names, index + 1, std::move(expanded), result);
    }
}

std::vector<std::string> numericRange(unsigned begin, unsigned end) {
    std::vector<std::string> values;
    for(unsigned value = begin; value <= end; ++value)
        values.push_back(std::to_string(value));
    return values;
}

} // namespace

std::vector<Entry> loadTagList(const std::string &path) {
    std::ifstream input(path);
    if(!input)
        throw std::runtime_error("cannot open DAPHNE tag-list registry: " + path);

    std::string line;
    if(!std::getline(input, line))
        throw std::runtime_error("empty DAPHNE tag-list registry: " + path);
    if(line.size() >= 3 && static_cast<unsigned char>(line[0]) == 0xEF &&
       static_cast<unsigned char>(line[1]) == 0xBB &&
       static_cast<unsigned char>(line[2]) == 0xBF)
        line.erase(0, 3);
    const auto header = parseCsvRecord(line);
    std::map<std::string, size_t> columns;
    for(size_t index = 0; index < header.size(); ++index)
        columns[trim(header[index])] = index;

    const std::vector<std::string> required = {
        "Subsystem", "Signal / Tag Name", "Description", "OPC-UA Node Address",
        "Data Type", "Eng Units", "DCS Access", "Notes", "Control Owner",
    };
    for(const auto &name : required) {
        if(columns.find(name) == columns.end())
            throw std::runtime_error("DAPHNE tag-list registry missing column: " + name);
    }

    std::vector<Entry> entries;
    std::set<std::string> patterns;
    size_t lineNo = 1;
    while(std::getline(input, line)) {
        ++lineNo;
        if(trim(line).empty())
            continue;
        const auto fields = parseCsvRecord(line);
        auto field = [&](const std::string &name) -> std::string {
            const size_t index = columns.at(name);
            if(index >= fields.size())
                throw std::runtime_error("short tag-list row " + std::to_string(lineNo));
            return trim(fields[index]);
        };

        Entry entry;
        entry.subsystem = field("Subsystem");
        entry.name = field("Signal / Tag Name");
        entry.description = field("Description");
        entry.nodeIdPattern = field("OPC-UA Node Address");
        entry.valueType = parseValueType(field("Data Type"));
        entry.engineeringUnits = field("Eng Units");
        entry.access = parseAccess(field("DCS Access"));
        entry.implementationStatus = implementationStatus(field("Notes"));
        entry.controlOwner = field("Control Owner");

        if(entry.subsystem.empty() || entry.name.empty() || entry.description.empty())
            throw std::runtime_error("incomplete tag-list row " + std::to_string(lineNo));
        (void)stringNodeId(entry.nodeIdPattern);
        if(entry.nodeIdPattern.find("{BoardId}") == std::string::npos)
            throw std::runtime_error("registry NodeId lacks {BoardId}: " +
                                     entry.nodeIdPattern);
        if(!patterns.insert(entry.nodeIdPattern).second)
            throw std::runtime_error("duplicate registry NodeId pattern: " +
                                     entry.nodeIdPattern);
        entries.push_back(std::move(entry));
    }
    return entries;
}

ExpansionConfig defaultExpansionConfig() {
    ExpansionConfig config;
    config.instances["BoardId"] = {"015"};
    config.instances["Afe"] = numericRange(0, 4);
    config.instances["Channel"] = numericRange(0, 39);
    config.instances["Fan"] = numericRange(0, 1);
    config.instances["Interface"] = {"eth0"};
    config.instances["DataLink"] = numericRange(0, 3);
    config.instances["Sfp"] = {"timing", "ccm", "data0", "data1", "data2", "data3"};
    config.instances["Bus"] = {"1", "2"};
    config.instances["Address"] = {
        "0x10", "0x17", "0x12", "0x16", "0x32", "0x36", "0x70", "0x71",
        "0x72", "0x40", "0x41", "0x42", "0x18", "0x50",
    };
    config.instances["Device"] = {"spidev3.0"};
    config.instances["Rail"] = {"3VD3", "2VA1", "3VA6", "1VD8"};
    config.instances["Sensor"] = {"soc", "carrier"};
    config.instances["Service"] = {
        "firmware.service", "clockchip.service", "endpoint.service", "hermes.service",
        "daphne.service", "daphne-boot-ok.service", "rpu",
    };
    config.i2cDevices = {
        {"1", "0x10"}, {"1", "0x17"}, {"2", "0x12"}, {"2", "0x16"},
        {"2", "0x32"}, {"2", "0x36"}, {"2", "0x70"}, {"2", "0x71"},
        {"2", "0x72"},
    };
    return config;
}

std::vector<ExpandedNode> expand(const std::vector<Entry> &entries,
                                 const ExpansionConfig &config) {
    std::vector<ExpandedNode> result;
    std::set<std::string> nodeIds;
    for(const auto &entry : entries) {
        const std::string id = stringNodeId(entry.nodeIdPattern);
        auto names = placeholders(id);
        const bool pairedI2c = std::find(names.begin(), names.end(), "Bus") != names.end() &&
                               std::find(names.begin(), names.end(), "Address") != names.end();
        std::vector<ExpandedNode> entryNodes;
        if(pairedI2c) {
            if(config.i2cDevices.empty())
                throw std::runtime_error("no controlled I2C bus/address pairs configured");
            names.erase(std::remove(names.begin(), names.end(), "Bus"), names.end());
            names.erase(std::remove(names.begin(), names.end(), "Address"), names.end());
            for(const auto &[bus, address] : config.i2cDevices) {
                std::string pairedId = id;
                replaceAll(pairedId, "{Bus}", bus);
                replaceAll(pairedId, "{Address}", address);
                expandRecursive(entry, config, names, 0, std::move(pairedId), entryNodes);
            }
        } else {
            expandRecursive(entry, config, names, 0, id, entryNodes);
        }
        for(auto &node : entryNodes) {
            if(!nodeIds.insert(node.nodeId).second)
                throw std::runtime_error("duplicate expanded registry NodeId: " + node.nodeId);
            result.push_back(std::move(node));
            if(result.size() > config.maximumNodes)
                throw std::runtime_error(
                    "registry expansion exceeds configured maximum node count");
        }
    }
    return result;
}

Summary summarize(const std::vector<Entry> &entries,
                  const std::vector<ExpandedNode> &expanded) {
    Summary summary;
    summary.patterns = entries.size();
    for(const auto &entry : entries) {
        if(entry.access == Access::ReadOnly)
            ++summary.readOnlyPatterns;
        else
            ++summary.writeOnlyPatterns;
    }
    for(const auto &node : expanded) {
        if(node.entry.access == Access::ReadOnly)
            ++summary.expandedReadNodes;
        else
            ++summary.expandedMethodNodes;
    }
    return summary;
}

std::vector<std::string> parseInstanceList(const std::string &value) {
    std::vector<std::string> result;
    std::istringstream input(value);
    std::string item;
    while(std::getline(input, item, ',')) {
        item = trim(item);
        if(item.empty())
            continue;
        const auto range = item.find("..");
        if(range == std::string::npos) {
            result.push_back(item);
            continue;
        }
        const unsigned begin = static_cast<unsigned>(std::stoul(trim(item.substr(0, range))));
        const unsigned end = static_cast<unsigned>(std::stoul(trim(item.substr(range + 2))));
        if(end < begin || end - begin > 10000)
            throw std::runtime_error("invalid registry instance range: " + item);
        const auto values = numericRange(begin, end);
        result.insert(result.end(), values.begin(), values.end());
    }
    if(result.empty())
        throw std::runtime_error("empty registry instance list");
    return result;
}

std::vector<std::pair<std::string, std::string>> parseI2cDevices(
    const std::string &value) {
    std::vector<std::pair<std::string, std::string>> result;
    for(const auto &item : parseInstanceList(value)) {
        const auto separator = item.find(':');
        if(separator == std::string::npos || separator == 0 || separator + 1 >= item.size())
            throw std::runtime_error("invalid registry I2C device pair: " + item);
        result.emplace_back(trim(item.substr(0, separator)),
                            trim(item.substr(separator + 1)));
    }
    return result;
}

std::string stringNodeId(const std::string &nodeIdPattern) {
    if(nodeIdPattern.rfind(kNamespacePrefix, 0) != 0)
        throw std::runtime_error("registry NodeId is outside urn:dune:pds:daphne: " +
                                 nodeIdPattern);
    const std::string result = nodeIdPattern.substr(
        std::char_traits<char>::length(kNamespacePrefix));
    if(result.empty())
        throw std::runtime_error("empty registry string NodeId");
    return result;
}

std::string valueTypeName(ValueType type) {
    switch(type) {
    case ValueType::Boolean: return "Boolean";
    case ValueType::Integer: return "Integer";
    case ValueType::Long: return "Long";
    case ValueType::Double: return "Double";
    case ValueType::String: return "String";
    case ValueType::DateTime: return "DateTime";
    }
    throw std::runtime_error("unknown registry value type");
}

} // namespace pds::registry
