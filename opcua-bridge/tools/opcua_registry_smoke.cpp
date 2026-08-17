#include <open62541/client.h>
#include <open62541/client_config_default.h>
#include <open62541/client_highlevel.h>

#include "registry.hpp"

#include <iostream>
#include <map>
#include <stdexcept>
#include <string>

namespace {

constexpr const char *kNamespaceUri = "urn:dune:pds:daphne";

std::string uaString(const UA_String &value) {
    return std::string(reinterpret_cast<const char *>(value.data), value.length);
}

UA_UInt16 findNamespace(UA_Client *client) {
    UA_Variant value;
    UA_Variant_init(&value);
    const UA_StatusCode rc = UA_Client_readValueAttribute(
        client, UA_NODEID_NUMERIC(0, UA_NS0ID_SERVER_NAMESPACEARRAY), &value);
    if(rc != UA_STATUSCODE_GOOD ||
       !UA_Variant_hasArrayType(&value, &UA_TYPES[UA_TYPES_STRING])) {
        UA_Variant_clear(&value);
        throw std::runtime_error("cannot read OPC-UA NamespaceArray");
    }
    UA_UInt16 result = 0;
    const auto *items = static_cast<const UA_String *>(value.data);
    for(size_t index = 0; index < value.arrayLength; ++index) {
        if(uaString(items[index]) == kNamespaceUri) {
            result = static_cast<UA_UInt16>(index);
            break;
        }
    }
    UA_Variant_clear(&value);
    if(result == 0)
        throw std::runtime_error(std::string("namespace not published: ") + kNamespaceUri);
    return result;
}

const UA_NodeId &expectedDataType(pds::registry::ValueType type) {
    switch(type) {
    case pds::registry::ValueType::Boolean: return UA_TYPES[UA_TYPES_BOOLEAN].typeId;
    case pds::registry::ValueType::Integer: return UA_TYPES[UA_TYPES_INT32].typeId;
    case pds::registry::ValueType::Long: return UA_TYPES[UA_TYPES_INT64].typeId;
    case pds::registry::ValueType::Double: return UA_TYPES[UA_TYPES_DOUBLE].typeId;
    case pds::registry::ValueType::String: return UA_TYPES[UA_TYPES_STRING].typeId;
    case pds::registry::ValueType::DateTime: return UA_TYPES[UA_TYPES_DATETIME].typeId;
    }
    throw std::runtime_error("unknown registry value type");
}

bool checkNode(UA_Client *client, UA_UInt16 ns,
               const pds::registry::ExpandedNode &expected,
               bool expectWritesDisabled) {
    UA_NodeId node = UA_NODEID_STRING_ALLOC(ns, expected.nodeId.c_str());
    UA_NodeClass nodeClass = UA_NODECLASS_UNSPECIFIED;
    UA_StatusCode rc = UA_Client_readNodeClassAttribute(client, node, &nodeClass);
    if(rc != UA_STATUSCODE_GOOD) {
        std::cerr << "missing " << expected.nodeId << ": " << UA_StatusCode_name(rc) << "\n";
        UA_NodeId_clear(&node);
        return false;
    }

    const UA_NodeClass wanted = expected.entry.access == pds::registry::Access::ReadOnly
                                    ? UA_NODECLASS_VARIABLE
                                    : UA_NODECLASS_METHOD;
    if(nodeClass != wanted) {
        std::cerr << "wrong node class " << expected.nodeId << "\n";
        UA_NodeId_clear(&node);
        return false;
    }

    bool ok = true;
    if(wanted == UA_NODECLASS_VARIABLE) {
        UA_NodeId dataType;
        UA_NodeId_init(&dataType);
        rc = UA_Client_readDataTypeAttribute(client, node, &dataType);
        if(rc != UA_STATUSCODE_GOOD ||
           !UA_NodeId_equal(&dataType, &expectedDataType(expected.entry.valueType))) {
            std::cerr << "wrong data type " << expected.nodeId << ": "
                      << UA_StatusCode_name(rc) << " expected="
                      << pds::registry::valueTypeName(expected.entry.valueType) << "\n";
            ok = false;
        }
        UA_NodeId_clear(&dataType);
    } else if(expectWritesDisabled) {
        UA_Boolean executable = true;
        rc = UA_Client_readExecutableAttribute(client, node, &executable);
        if(rc != UA_STATUSCODE_GOOD || executable) {
            std::cerr << "method unexpectedly executable " << expected.nodeId << ": "
                      << UA_StatusCode_name(rc) << "\n";
            ok = false;
        }
    }
    UA_NodeId_clear(&node);
    return ok;
}

bool checkValueStatus(UA_Client *client, UA_UInt16 ns, const std::string &id,
                      UA_StatusCode expected) {
    UA_NodeId node = UA_NODEID_STRING_ALLOC(ns, id.c_str());
    UA_Variant value;
    UA_Variant_init(&value);
    const UA_StatusCode status = UA_Client_readValueAttribute(client, node, &value);
    UA_Variant_clear(&value);
    UA_NodeId_clear(&node);
    if(status == expected)
        return true;
    std::cerr << "unexpected value status " << id << ": got="
              << UA_StatusCode_name(status) << " expected="
              << UA_StatusCode_name(expected) << "\n";
    return false;
}

} // namespace

int main(int argc, char **argv) {
    if(argc != 4 && argc != 5) {
        std::cerr << "usage: pds-opcua-registry-smoke ENDPOINT TAG_LIST.csv BOARD_ID "
                     "[--expect-writes-disabled]\n";
        return 2;
    }
    const bool expectWritesDisabled = argc == 5 &&
                                      std::string(argv[4]) == "--expect-writes-disabled";
    if(argc == 5 && !expectWritesDisabled) {
        std::cerr << "unknown option: " << argv[4] << "\n";
        return 2;
    }

    try {
        auto expansion = pds::registry::defaultExpansionConfig();
        expansion.instances["BoardId"] = {argv[3]};
        const auto entries = pds::registry::loadTagList(argv[2]);
        const auto expectedNodes = pds::registry::expand(entries, expansion);

        UA_Client *client = UA_Client_new();
        UA_ClientConfig_setDefault(UA_Client_getConfig(client));
        const UA_StatusCode connect = UA_Client_connect(client, argv[1]);
        if(connect != UA_STATUSCODE_GOOD) {
            std::cerr << "connect failed: " << UA_StatusCode_name(connect) << "\n";
            UA_Client_delete(client);
            return 1;
        }

        const UA_UInt16 ns = findNamespace(client);
        size_t checked = 0;
        size_t failures = 0;
        for(const auto &node : expectedNodes) {
            if(!checkNode(client, ns, node, expectWritesDisabled))
                ++failures;
            ++checked;
        }
        const std::string base = "DAPHNE.Boards." + std::string(argv[3]) + ".";
        if(!checkValueStatus(client, ns, base + "Status.Connected",
                             UA_STATUSCODE_GOOD))
            ++failures;
        if(!checkValueStatus(client, ns, base + "Bridge.Version",
                             UA_STATUSCODE_GOOD))
            ++failures;
        if(!checkValueStatus(client, ns, base + "Authority.ExternalActivityPermit",
                             UA_STATUSCODE_BADWAITINGFORINITIALDATA))
            ++failures;
        UA_Client_disconnect(client);
        UA_Client_delete(client);

        const auto summary = pds::registry::summarize(entries, expectedNodes);
        std::cout << "namespace=" << kNamespaceUri << " ns=" << ns
                  << " checked=" << checked << " failures=" << failures
                  << " read_nodes=" << summary.expandedReadNodes
                  << " method_nodes=" << summary.expandedMethodNodes << "\n";
        return failures == 0 ? 0 : 1;
    } catch(const std::exception &err) {
        std::cerr << "OPC-UA registry smoke failed: " << err.what() << "\n";
        return 1;
    }
}
