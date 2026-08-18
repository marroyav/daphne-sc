#include "v8_snapshot.hpp"

#include <cstdlib>
#include <iostream>
#include <stdexcept>
#include <string>

namespace {

using Response = daphne::telemetry::v8::ReadTelemetrySnapshotResponse;

void Require(bool condition, const std::string& message) {
  if (!condition) {
    std::cerr << "FAILED: " << message << '\n';
    std::exit(1);
  }
}

Response MakeValidResponse() {
  Response response;
  response.set_success(true);
  response.set_schema_major(1);
  response.set_schema_minor(0);
  response.set_schema_source_sha256(std::string(64, 'a'));
  response.set_contract_revision("PDS-DAQ-ICD-proposed-v8-test");
  response.set_board_id("015");
  response.set_request_sequence(42);
  response.set_snapshot_time_unix_ns(123);
  auto* point = response.add_points();
  point->set_node_id("DAPHNE.Boards.015.Firmware.Loaded");
  point->set_quality(daphne::telemetry::v8::TELEMETRY_QUALITY_GOOD);
  point->set_boolean_value(true);
  return response;
}

std::string Serialize(const Response& response) {
  std::string payload;
  Require(response.SerializeToString(&payload), "test response serializes");
  return payload;
}

template <typename Change>
void RequireRejected(Change change, const std::string& message) {
  auto response = MakeValidResponse();
  change(response);
  try {
    (void)pds::bridge::v8::ParseAndValidate(Serialize(response), "015", 42);
  } catch (const std::runtime_error&) {
    return;
  }
  Require(false, message);
}

}  // namespace

int main() {
  const auto request = pds::bridge::v8::MakeRequest(42);
  Require(request.request_sequence() == 42 && request.include_unavailable(),
          "request asks for the complete correlated snapshot");

  auto snapshot = pds::bridge::v8::ParseAndValidate(Serialize(MakeValidResponse()), "015", 42);
  Require(
      snapshot.good_count == 1 && snapshot.invalid_count == 0 && snapshot.unavailable_count == 0,
      "valid snapshot is counted");
  const auto* loaded = snapshot.GoodPoint("Firmware.Loaded");
  Require(loaded != nullptr && loaded->boolean_value(), "good point lookup works");

  try {
    (void)pds::bridge::v8::ParseAndValidate("not protobuf", "015", 42);
    Require(false, "malformed protobuf must be rejected");
  } catch (const std::runtime_error&) {
  }

  RequireRejected([](Response& response) { response.set_success(false); },
                  "failed response must be rejected");
  RequireRejected([](Response& response) { response.set_schema_major(2); },
                  "unsupported schema major must be rejected");
  RequireRejected([](Response& response) { response.set_schema_source_sha256("bad"); },
                  "malformed schema hash must be rejected");
  RequireRejected([](Response& response) { response.clear_contract_revision(); },
                  "missing contract revision must be rejected");
  RequireRejected([](Response& response) { response.set_board_id("016"); },
                  "wrong board must be rejected");
  RequireRejected([](Response& response) { response.set_request_sequence(43); },
                  "wrong request sequence must be rejected");
  RequireRejected([](Response& response) { response.clear_points(); },
                  "empty snapshot must be rejected");
  RequireRejected(
      [](Response& response) {
        response.mutable_points(0)->set_node_id("DAPHNE.Boards.016.Firmware.Loaded");
      },
      "point outside the configured board must be rejected");
  RequireRejected([](Response& response) { *response.add_points() = response.points(0); },
                  "duplicate NodeId must be rejected");
  RequireRejected([](Response& response) { response.mutable_points(0)->clear_value(); },
                  "Good point without a value must be rejected");
  RequireRejected(
      [](Response& response) {
        response.mutable_points(0)->set_quality(
            daphne::telemetry::v8::TELEMETRY_QUALITY_UNAVAILABLE);
      },
      "Unavailable point carrying a value must be rejected");
  RequireRejected(
      [](Response& response) {
        response.mutable_points(0)->set_quality(
            daphne::telemetry::v8::TELEMETRY_QUALITY_UNSPECIFIED);
      },
      "unspecified quality must be rejected");

  std::cout << "v8 snapshot validation tests passed\n";
  return 0;
}
