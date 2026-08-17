#include <open62541/client.h>
#include <open62541/client_config_default.h>
#include <open62541/client_highlevel.h>

#include <cstdint>
#include <cstdlib>
#include <iomanip>
#include <iostream>
#include <sstream>
#include <string>
#include <vector>

namespace {

constexpr const char *kDefaultNamespaceUri = "urn:dune:pds:np04";

std::string uaStringToStd(const UA_String &value) {
    return std::string(reinterpret_cast<const char *>(value.data), value.length);
}

std::string hex64(UA_UInt64 value) {
    std::ostringstream out;
    out << "0x" << std::uppercase << std::hex << std::setw(16)
        << std::setfill('0') << static_cast<uint64_t>(value);
    return out.str();
}

bool findNamespace(UA_Client *client, const std::string &namespaceUri, UA_UInt16 &ns) {
    UA_Variant value;
    UA_Variant_init(&value);
    const UA_StatusCode rc = UA_Client_readValueAttribute(
        client, UA_NODEID_NUMERIC(0, UA_NS0ID_SERVER_NAMESPACEARRAY), &value);
    if(rc != UA_STATUSCODE_GOOD || !UA_Variant_hasArrayType(&value, &UA_TYPES[UA_TYPES_STRING])) {
        std::cerr << "NamespaceArray read failed: " << UA_StatusCode_name(rc) << "\n";
        UA_Variant_clear(&value);
        return false;
    }

    const auto *items = static_cast<const UA_String *>(value.data);
    for(size_t i = 0; i < value.arrayLength; ++i) {
        const std::string uri = uaStringToStd(items[i]);
        std::cout << "ns=" << i << " " << uri << "\n";
        if(uri == namespaceUri)
            ns = static_cast<UA_UInt16>(i);
    }
    UA_Variant_clear(&value);
    return ns != 0;
}

bool printVariant(const UA_Variant &value) {
    if(UA_Variant_hasScalarType(&value, &UA_TYPES[UA_TYPES_BOOLEAN])) {
        std::cout << (*static_cast<const UA_Boolean *>(value.data) ? "true" : "false");
    } else if(UA_Variant_hasScalarType(&value, &UA_TYPES[UA_TYPES_STRING])) {
        std::cout << uaStringToStd(*static_cast<const UA_String *>(value.data));
    } else if(UA_Variant_hasScalarType(&value, &UA_TYPES[UA_TYPES_DOUBLE])) {
        std::cout << *static_cast<const UA_Double *>(value.data);
    } else if(UA_Variant_hasScalarType(&value, &UA_TYPES[UA_TYPES_UINT64])) {
        const auto v = *static_cast<const UA_UInt64 *>(value.data);
        std::cout << v << " (" << hex64(v) << ")";
    } else if(UA_Variant_hasScalarType(&value, &UA_TYPES[UA_TYPES_INT32])) {
        std::cout << *static_cast<const UA_Int32 *>(value.data);
    } else if(UA_Variant_hasScalarType(&value, &UA_TYPES[UA_TYPES_INT64])) {
        std::cout << *static_cast<const UA_Int64 *>(value.data);
    } else if(UA_Variant_hasScalarType(&value, &UA_TYPES[UA_TYPES_DATETIME])) {
        std::cout << *static_cast<const UA_DateTime *>(value.data);
    } else {
        std::cout << "<unsupported type>";
        return false;
    }
    return true;
}

bool readNode(UA_Client *client, UA_UInt16 ns, const std::string &id) {
    UA_NodeId node = UA_NODEID_STRING_ALLOC(ns, id.c_str());
    UA_Variant value;
    UA_Variant_init(&value);
    const UA_StatusCode rc = UA_Client_readValueAttribute(client, node, &value);
    UA_NodeId_clear(&node);

    std::cout << id << ": " << UA_StatusCode_name(rc) << " = ";
    if(rc != UA_STATUSCODE_GOOD) {
        std::cout << "\n";
        UA_Variant_clear(&value);
        return false;
    }

    const bool ok = printVariant(value);
    std::cout << "\n";
    UA_Variant_clear(&value);
    return ok;
}

std::vector<std::string> defaultNodes() {
    return {
        "PDS.NP04.Bridge.Version",
        "PDS.NP04.Bridge.BoardCount",
        "PDS.NP04.Summary.DaphneReady",
        "PDS.NP04.Summary.DaphneReadyCount",
        "PDS.NP04.Summary.DaphneTotalCount",
        "PDS.NP04.Summary.Ready",
        "PDS.NP04.Summary.Message",
        "PDS.NP04.DAPHNE.001.Status.Success",
        "PDS.NP04.DAPHNE.001.Status.TestRegHex",
        "PDS.NP04.DAPHNE.002.Status.Success",
        "PDS.NP04.DAPHNE.002.Status.TestRegHex",
        "PDS.NP04.DAPHNE.003.Status.Success",
        "PDS.NP04.DAPHNE.003.Status.TestRegHex",
        "PDS.NP04.DAPHNE.004.Status.Success",
        "PDS.NP04.DAPHNE.004.Status.TestRegHex",
        "PDS.NP04.PowerSupply.USB0.Status.Message",
    };
}

} // namespace

int main(int argc, char **argv) {
    const std::string endpoint = argc > 1 ? argv[1] : "opc.tcp://localhost:4840";
    const char *namespaceEnv = std::getenv("PDS_OPCUA_NAMESPACE");
    const std::string namespaceUri = namespaceEnv ? namespaceEnv : kDefaultNamespaceUri;
    std::vector<std::string> nodes;
    for(int i = 2; i < argc; ++i)
        nodes.emplace_back(argv[i]);
    if(nodes.empty())
        nodes = defaultNodes();

    UA_Client *client = UA_Client_new();
    UA_ClientConfig_setDefault(UA_Client_getConfig(client));
    UA_StatusCode rc = UA_Client_connect(client, endpoint.c_str());
    if(rc != UA_STATUSCODE_GOOD) {
        std::cerr << "connect failed: " << UA_StatusCode_name(rc) << "\n";
        UA_Client_delete(client);
        return 2;
    }

    UA_UInt16 ns = 0;
    bool ok = findNamespace(client, namespaceUri, ns);
    if(ok) {
        std::cout << "using PDS namespace " << namespaceUri << " ns=" << ns << "\n";
        for(const auto &node : nodes)
            ok = readNode(client, ns, node) && ok;
    }

    UA_Client_disconnect(client);
    UA_Client_delete(client);
    return ok ? 0 : 1;
}
