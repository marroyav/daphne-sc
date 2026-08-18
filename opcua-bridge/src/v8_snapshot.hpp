#ifndef DUNE_DAPHNE_SC_OPCUA_BRIDGE_SRC_V8_SNAPSHOT_HPP_
#define DUNE_DAPHNE_SC_OPCUA_BRIDGE_SRC_V8_SNAPSHOT_HPP_

#include <cstddef>
#include <cstdint>
#include <functional>
#include <string>

#include "daphne_v8_telemetry.pb.h"

namespace pds::bridge::v8 {

enum class WireValueType {
  Boolean,
  Integer,
  Long,
  Double,
  String,
  DateTime,
};

struct WireSampleView {
  std::string node_id;
  WireValueType value_type;
  const google::protobuf::Message* sample;
  const google::protobuf::FieldDescriptor* value_field;
  const daphne::telemetry::v8::SampleMetadata* metadata;

  bool HasValue() const;
};

using WireSampleVisitor = std::function<void(const WireSampleView&)>;

struct ValidatedSnapshot {
  daphne::telemetry::v8::ReadTelemetrySnapshotResponse response;
  size_t sample_count = 0;
  size_t good_count = 0;
  size_t unavailable_count = 0;
  size_t invalid_count = 0;
};

// Builds the one request used by the OPC-UA bridge.
daphne::telemetry::v8::ReadTelemetrySnapshotRequest MakeRequest(uint64_t sequence);

// SHA-256 of the exact canonical schema compiled into this bridge.
const char* ExpectedSchemaSourceSha256();

// Decodes and validates the complete board response before OPC-UA sees it.
// Throws std::runtime_error when the wire contract is violated.
ValidatedSnapshot ParseAndValidate(const std::string& payload, const std::string& board_id,
                                   uint64_t request_sequence);

// Visits the explicitly declared BoardTelemetry fields. NodeIds, instance
// placeholders, and wire types come exclusively from the protobuf schema.
void VisitWireSamples(const daphne::telemetry::v8::BoardTelemetry& telemetry,
                      const std::string& board_id, const WireSampleVisitor& visitor);

}  // namespace pds::bridge::v8

#endif  // DUNE_DAPHNE_SC_OPCUA_BRIDGE_SRC_V8_SNAPSHOT_HPP_
