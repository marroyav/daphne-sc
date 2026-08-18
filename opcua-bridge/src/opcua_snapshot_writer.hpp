#ifndef DUNE_DAPHNE_SC_OPCUA_BRIDGE_SRC_OPCUA_SNAPSHOT_WRITER_HPP_
#define DUNE_DAPHNE_SC_OPCUA_BRIDGE_SRC_OPCUA_SNAPSHOT_WRITER_HPP_

#include <open62541/server.h>

#include <cstddef>
#include <map>
#include <string>

#include "daphne_v8_telemetry.pb.h"
#include "registry.hpp"

namespace pds::bridge {

// Converts one validated protobuf snapshot into typed OPC-UA DataValues.
// The writer knows nothing about polling, ZMQ, configuration, or legacy data.
class OpcUaSnapshotWriter {
 public:
  OpcUaSnapshotWriter(UA_Server* server, UA_UInt16 namespace_index,
                      const std::map<std::string, pds::registry::ValueType>& node_types);

  // Returns the number of points rejected at the OPC-UA boundary.
  size_t Publish(const daphne::telemetry::v8::ReadTelemetrySnapshotResponse& snapshot,
                 bool transport_is_current) const;

 private:
  UA_Server* server_;
  UA_UInt16 namespace_index_;
  const std::map<std::string, pds::registry::ValueType>& node_types_;
};

}  // namespace pds::bridge

#endif  // DUNE_DAPHNE_SC_OPCUA_BRIDGE_SRC_OPCUA_SNAPSHOT_WRITER_HPP_
