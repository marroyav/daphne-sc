#include "opcua_snapshot_writer.hpp"

#include <cstdint>
#include <iostream>
#include <set>
#include <stdexcept>
#include <utility>

namespace pds::bridge {
namespace {

using daphne::telemetry::v8::TelemetryPoint;
using daphne::telemetry::v8::TelemetryQuality;

UA_DateTime UnixNsToUaDateTime(uint64_t unix_ns) {
  constexpr int64_t kUnixEpochInUaTicks = 116444736000000000LL;
  return static_cast<UA_DateTime>(unix_ns / 100ULL) + kUnixEpochInUaTicks;
}

UA_NodeId StringNodeId(UA_UInt16 namespace_index, const std::string& node_id) {
  return UA_NODEID_STRING(namespace_index, const_cast<char*>(node_id.c_str()));
}

template <typename Value>
UA_Variant MakeScalarVariant(const Value& value, size_t ua_type_index) {
  UA_Variant variant;
  UA_Variant_init(&variant);
  UA_Variant_setScalarCopy(&variant, &value, &UA_TYPES[ua_type_index]);
  return variant;
}

UA_Variant MakeStringVariant(const std::string& value) {
  UA_String ua_value = UA_STRING(const_cast<char*>(value.c_str()));
  return MakeScalarVariant(ua_value, UA_TYPES_STRING);
}

UA_Variant MakeInitialValue(pds::registry::ValueType type) {
  switch (type) {
    case pds::registry::ValueType::Boolean:
      return MakeScalarVariant(UA_Boolean{false}, UA_TYPES_BOOLEAN);
    case pds::registry::ValueType::Integer:
      return MakeScalarVariant(UA_Int32{0}, UA_TYPES_INT32);
    case pds::registry::ValueType::Long:
      return MakeScalarVariant(UA_Int64{0}, UA_TYPES_INT64);
    case pds::registry::ValueType::Double:
      return MakeScalarVariant(UA_Double{0.0}, UA_TYPES_DOUBLE);
    case pds::registry::ValueType::String:
      return MakeStringVariant("Unavailable");
    case pds::registry::ValueType::DateTime:
      return MakeScalarVariant(UA_DateTime{0}, UA_TYPES_DATETIME);
  }
  throw std::runtime_error("unknown registry value type");
}

UA_StatusCode QualityToStatus(TelemetryQuality quality) {
  switch (quality) {
    case daphne::telemetry::v8::TELEMETRY_QUALITY_GOOD:
      return UA_STATUSCODE_GOOD;
    case daphne::telemetry::v8::TELEMETRY_QUALITY_STALE:
      return UA_STATUSCODE_UNCERTAINLASTUSABLEVALUE;
    case daphne::telemetry::v8::TELEMETRY_QUALITY_INVALID:
      return UA_STATUSCODE_BADINVALIDSTATE;
    case daphne::telemetry::v8::TELEMETRY_QUALITY_UNAVAILABLE:
      return UA_STATUSCODE_BADWAITINGFORINITIALDATA;
    case daphne::telemetry::v8::TELEMETRY_QUALITY_NOT_APPLICABLE:
      return UA_STATUSCODE_BADNOTSUPPORTED;
    case daphne::telemetry::v8::TELEMETRY_QUALITY_UNSPECIFIED:
    default:
      return UA_STATUSCODE_BADINVALIDSTATE;
  }
}

UA_Variant PointToVariant(const TelemetryPoint& point, pds::registry::ValueType expected_type,
                          bool* type_matches) {
  *type_matches = true;
  switch (expected_type) {
    case pds::registry::ValueType::Boolean:
      if (point.value_case() == TelemetryPoint::kBooleanValue) {
        return MakeScalarVariant(static_cast<UA_Boolean>(point.boolean_value()), UA_TYPES_BOOLEAN);
      }
      break;
    case pds::registry::ValueType::Integer:
      if (point.value_case() == TelemetryPoint::kIntegerValue) {
        return MakeScalarVariant(static_cast<UA_Int32>(point.integer_value()), UA_TYPES_INT32);
      }
      break;
    case pds::registry::ValueType::Long:
      if (point.value_case() == TelemetryPoint::kLongValue) {
        return MakeScalarVariant(static_cast<UA_Int64>(point.long_value()), UA_TYPES_INT64);
      }
      break;
    case pds::registry::ValueType::Double:
      if (point.value_case() == TelemetryPoint::kDoubleValue) {
        return MakeScalarVariant(static_cast<UA_Double>(point.double_value()), UA_TYPES_DOUBLE);
      }
      break;
    case pds::registry::ValueType::String:
      if (point.value_case() == TelemetryPoint::kStringValue) {
        return MakeStringVariant(point.string_value());
      }
      break;
    case pds::registry::ValueType::DateTime:
      if (point.value_case() == TelemetryPoint::kDatetimeUnixNs && point.datetime_unix_ns() >= 0) {
        return MakeScalarVariant(
            UnixNsToUaDateTime(static_cast<uint64_t>(point.datetime_unix_ns())), UA_TYPES_DATETIME);
      }
      break;
  }

  if (point.value_case() != TelemetryPoint::VALUE_NOT_SET) {
    *type_matches = false;
  }
  return MakeInitialValue(expected_type);
}

void WriteDataValue(UA_Server* server, UA_UInt16 namespace_index, const std::string& node_id,
                    UA_Variant value, UA_StatusCode status, UA_DateTime source_timestamp) {
  UA_DataValue data_value;
  UA_DataValue_init(&data_value);
  data_value.hasValue = true;
  data_value.value = value;
  data_value.hasStatus = true;
  data_value.status = status;
  if (source_timestamp != 0) {
    data_value.hasSourceTimestamp = true;
    data_value.sourceTimestamp = source_timestamp;
  }

  const UA_StatusCode result =
      UA_Server_writeDataValue(server, StringNodeId(namespace_index, node_id), data_value);
  UA_Variant_clear(&value);
  if (result != UA_STATUSCODE_GOOD) {
    std::cerr << "warning: failed to write OPC-UA telemetry node " << node_id << '\n';
  }
}

}  // namespace

OpcUaSnapshotWriter::OpcUaSnapshotWriter(
    UA_Server* server, UA_UInt16 namespace_index,
    const std::map<std::string, pds::registry::ValueType>& node_types)
    : server_(server), namespace_index_(namespace_index), node_types_(node_types) {
  if (server_ == nullptr) {
    throw std::invalid_argument("OPC-UA snapshot writer requires a server");
  }
}

size_t OpcUaSnapshotWriter::Publish(
    const daphne::telemetry::v8::ReadTelemetrySnapshotResponse& snapshot,
    bool transport_is_current) const {
  size_t rejected_points = 0;
  std::set<std::string> seen_node_ids;

  for (const TelemetryPoint& point : snapshot.points()) {
    if (!seen_node_ids.insert(point.node_id()).second) {
      ++rejected_points;
      continue;
    }

    const auto expected_type = node_types_.find(point.node_id());
    if (expected_type == node_types_.end()) {
      ++rejected_points;
      continue;
    }

    bool type_matches = true;
    UA_Variant value = PointToVariant(point, expected_type->second, &type_matches);
    UA_StatusCode status = QualityToStatus(point.quality());
    const bool good_value_is_missing =
        point.quality() == daphne::telemetry::v8::TELEMETRY_QUALITY_GOOD &&
        point.value_case() == TelemetryPoint::VALUE_NOT_SET;
    if (!type_matches || good_value_is_missing) {
      status = UA_STATUSCODE_BADTYPEMISMATCH;
      ++rejected_points;
    } else if (!transport_is_current && status == UA_STATUSCODE_GOOD) {
      status = UA_STATUSCODE_UNCERTAINLASTUSABLEVALUE;
    }

    const uint64_t source_time_ns = point.sample_time_unix_ns() != 0
                                        ? point.sample_time_unix_ns()
                                        : snapshot.snapshot_time_unix_ns();
    WriteDataValue(server_, namespace_index_, point.node_id(), std::move(value), status,
                   source_time_ns == 0 ? 0 : UnixNsToUaDateTime(source_time_ns));
  }

  return rejected_points;
}

}  // namespace pds::bridge
