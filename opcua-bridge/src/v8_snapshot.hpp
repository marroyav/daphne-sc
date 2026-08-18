#ifndef DUNE_DAPHNE_SC_OPCUA_BRIDGE_SRC_V8_SNAPSHOT_HPP_
#define DUNE_DAPHNE_SC_OPCUA_BRIDGE_SRC_V8_SNAPSHOT_HPP_

#include <cstddef>
#include <cstdint>
#include <string>

#include "daphne_v8_telemetry.pb.h"

namespace pds::bridge::v8 {

struct ValidatedSnapshot {
  daphne::telemetry::v8::ReadTelemetrySnapshotResponse response;
  size_t good_count = 0;
  size_t unavailable_count = 0;
  size_t invalid_count = 0;

  const daphne::telemetry::v8::TelemetryPoint* GoodPoint(const std::string& suffix) const;
};

// Builds the one request used by the OPC-UA bridge.
daphne::telemetry::v8::ReadTelemetrySnapshotRequest MakeRequest(uint64_t sequence);

// Decodes and validates the complete board response before OPC-UA sees it.
// Throws std::runtime_error when the wire contract is violated.
ValidatedSnapshot ParseAndValidate(const std::string& payload, const std::string& board_id,
                                   uint64_t request_sequence);

}  // namespace pds::bridge::v8

#endif  // DUNE_DAPHNE_SC_OPCUA_BRIDGE_SRC_V8_SNAPSHOT_HPP_
