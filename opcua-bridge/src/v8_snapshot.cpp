#include "v8_snapshot.hpp"

#include <algorithm>
#include <cctype>
#include <set>
#include <stdexcept>
#include <utility>

namespace pds::bridge::v8 {
namespace {

using daphne::telemetry::v8::TELEMETRY_QUALITY_GOOD;
using daphne::telemetry::v8::TELEMETRY_QUALITY_INVALID;
using daphne::telemetry::v8::TELEMETRY_QUALITY_NOT_APPLICABLE;
using daphne::telemetry::v8::TELEMETRY_QUALITY_STALE;
using daphne::telemetry::v8::TELEMETRY_QUALITY_UNAVAILABLE;
using Point = daphne::telemetry::v8::TelemetryPoint;

std::string BoardPrefix(const std::string& board_id) { return "DAPHNE.Boards." + board_id + "."; }

bool CarriesValue(const Point& point) { return point.value_case() != Point::VALUE_NOT_SET; }

}  // namespace

daphne::telemetry::v8::ReadTelemetrySnapshotRequest MakeRequest(uint64_t sequence) {
  daphne::telemetry::v8::ReadTelemetrySnapshotRequest request;
  request.set_detail(daphne::telemetry::v8::TELEMETRY_DETAIL_STANDARD);
  request.set_request_sequence(sequence);
  request.set_include_unavailable(true);
  return request;
}

ValidatedSnapshot ParseAndValidate(const std::string& payload, const std::string& board_id,
                                   uint64_t request_sequence) {
  ValidatedSnapshot result;
  auto& response = result.response;
  if (!response.ParseFromString(payload))
    throw std::runtime_error("bad ReadTelemetrySnapshotResponse payload");
  if (!response.success())
    throw std::runtime_error("telemetry response failed: " + response.message());
  if (response.schema_major() != 1)
    throw std::runtime_error("unsupported telemetry schema major " +
                             std::to_string(response.schema_major()));
  if (response.schema_source_sha256().size() != 64 ||
      !std::all_of(response.schema_source_sha256().begin(), response.schema_source_sha256().end(),
                   [](unsigned char value) { return std::isxdigit(value) != 0; }))
    throw std::runtime_error("missing or malformed telemetry schema source hash");
  if (response.contract_revision().empty())
    throw std::runtime_error("telemetry response has no contract revision");
  if (response.board_id() != board_id)
    throw std::runtime_error("telemetry board_id mismatch: expected " + board_id + ", got " +
                             response.board_id());
  if (response.request_sequence() != request_sequence)
    throw std::runtime_error("telemetry request_sequence mismatch");
  if (response.points_size() == 0)
    throw std::runtime_error("telemetry response contains no points");

  std::set<std::string> node_ids;
  const std::string prefix = BoardPrefix(board_id);
  for (const auto& point : response.points()) {
    if (point.node_id().rfind(prefix, 0) != 0)
      throw std::runtime_error("telemetry point is outside configured board: " + point.node_id());
    if (!node_ids.insert(point.node_id()).second)
      throw std::runtime_error("duplicate telemetry NodeId: " + point.node_id());

    const bool has_value = CarriesValue(point);
    if ((point.quality() == TELEMETRY_QUALITY_GOOD || point.quality() == TELEMETRY_QUALITY_STALE) &&
        !has_value)
      throw std::runtime_error("telemetry point lacks value: " + point.node_id());
    if ((point.quality() == TELEMETRY_QUALITY_UNAVAILABLE ||
         point.quality() == TELEMETRY_QUALITY_NOT_APPLICABLE) &&
        has_value)
      throw std::runtime_error("unavailable telemetry point carries a value: " + point.node_id());

    switch (point.quality()) {
      case TELEMETRY_QUALITY_GOOD:
        ++result.good_count;
        break;
      case TELEMETRY_QUALITY_STALE:
      case TELEMETRY_QUALITY_NOT_APPLICABLE:
        break;
      case TELEMETRY_QUALITY_UNAVAILABLE:
        ++result.unavailable_count;
        break;
      case TELEMETRY_QUALITY_INVALID:
        ++result.invalid_count;
        break;
      case daphne::telemetry::v8::TELEMETRY_QUALITY_UNSPECIFIED:
      default:
        throw std::runtime_error("unsupported telemetry quality for point: " + point.node_id());
    }
  }
  return result;
}

const daphne::telemetry::v8::TelemetryPoint* ValidatedSnapshot::GoodPoint(
    const std::string& suffix) const {
  const std::string id = BoardPrefix(response.board_id()) + suffix;
  for (const auto& point : response.points()) {
    if (point.node_id() == id && point.quality() == TELEMETRY_QUALITY_GOOD) return &point;
  }
  return nullptr;
}

}  // namespace pds::bridge::v8
