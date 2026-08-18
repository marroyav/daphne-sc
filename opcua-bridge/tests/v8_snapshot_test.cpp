#include "v8_snapshot.hpp"

#include <google/protobuf/descriptor.h>

#include <cstdlib>
#include <iostream>
#include <limits>
#include <stdexcept>
#include <string>

namespace {

using google::protobuf::FieldDescriptor;
using google::protobuf::Message;
using Response = daphne::telemetry::v8::ReadTelemetrySnapshotResponse;

void Require(bool condition, const std::string& message) {
  if (!condition) {
    std::cerr << "FAILED: " << message << '\n';
    std::exit(1);
  }
}

std::string InstanceValue(const std::string& field_name) {
  if (field_name == "afe" || field_name == "channel" || field_name == "fan" ||
      field_name == "data_link") {
    return "0";
  }
  if (field_name == "bus") return "1";
  if (field_name == "address") return "0x10";
  if (field_name == "interface") return "eth0";
  if (field_name == "service") return "daphne.service";
  if (field_name == "device") return "spidev3.0";
  if (field_name == "rail") return "3VD3";
  if (field_name == "sensor") return "soc";
  if (field_name == "sfp") return "timing";
  throw std::runtime_error("test has no instance value for " + field_name);
}

Message* AddExplicitSample(daphne::telemetry::v8::BoardTelemetry* telemetry,
                           const FieldDescriptor* field) {
  const auto* reflection = telemetry->GetReflection();
  if (!field->is_repeated()) return reflection->MutableMessage(telemetry, field);

  Message* entry = reflection->AddMessage(telemetry, field);
  const auto* entry_descriptor = entry->GetDescriptor();
  const auto* entry_reflection = entry->GetReflection();
  const FieldDescriptor* sample_field = entry_descriptor->FindFieldByName("sample");
  Require(sample_field != nullptr, "indexed entry declares sample");
  for (int index = 0; index < entry_descriptor->field_count(); ++index) {
    const FieldDescriptor* key = entry_descriptor->field(index);
    if (key == sample_field) continue;
    Require(key->cpp_type() == FieldDescriptor::CPPTYPE_STRING, "indexed entry key is a string");
    entry_reflection->SetString(entry, key, InstanceValue(key->name()));
  }
  return entry_reflection->MutableMessage(entry, sample_field);
}

daphne::telemetry::v8::SampleMetadata* MutableMetadata(Message* sample) {
  const FieldDescriptor* metadata_field = sample->GetDescriptor()->FindFieldByName("metadata");
  Require(metadata_field != nullptr, "sample declares metadata");
  Message* metadata_message = sample->GetReflection()->MutableMessage(sample, metadata_field);
  auto* metadata = dynamic_cast<daphne::telemetry::v8::SampleMetadata*>(metadata_message);
  Require(metadata != nullptr, "sample metadata has the declared type");
  return metadata;
}

void ClearSampleValue(Message* sample) {
  const FieldDescriptor* value = sample->GetDescriptor()->FindFieldByName("value");
  Require(value != nullptr, "sample value field exists");
  sample->GetReflection()->ClearField(sample, value);
}

Response MakeValidResponse() {
  Response response;
  response.set_success(true);
  response.set_schema_major(2);
  response.set_schema_minor(0);
  response.set_schema_source_sha256(pds::bridge::v8::ExpectedSchemaSourceSha256());
  response.set_contract_revision("PDS-DAQ-ICD-proposed-v8-explicit-test");
  response.set_board_id("015");
  response.set_request_sequence(42);
  response.set_snapshot_time_unix_ns(123);

  auto* telemetry = response.mutable_telemetry();
  const auto* descriptor = telemetry->GetDescriptor();
  for (int index = 0; index < descriptor->field_count(); ++index) {
    Message* sample = AddExplicitSample(telemetry, descriptor->field(index));
    auto* metadata = MutableMetadata(sample);
    metadata->set_quality(daphne::telemetry::v8::TELEMETRY_QUALITY_UNAVAILABLE);
    metadata->set_sample_time_unix_ns(123);
    metadata->set_sample_monotonic_ns(456);
  }
  auto* loaded = telemetry->mutable_firmware_loaded();
  loaded->mutable_metadata()->set_quality(daphne::telemetry::v8::TELEMETRY_QUALITY_GOOD);
  loaded->set_value(true);
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
  Require(request.request_sequence() == 42, "request carries only snapshot correlation");

  auto snapshot = pds::bridge::v8::ParseAndValidate(Serialize(MakeValidResponse()), "015", 42);
  Require(snapshot.sample_count == 316 && snapshot.good_count == 1 && snapshot.invalid_count == 0 &&
              snapshot.unavailable_count == 315,
          "valid explicit snapshot is counted");
  Require(snapshot.response.telemetry().firmware_loaded().value(),
          "named firmware_loaded field is directly accessible");

  try {
    (void)pds::bridge::v8::ParseAndValidate("not protobuf", "015", 42);
    Require(false, "malformed protobuf must be rejected");
  } catch (const std::runtime_error&) {
  }

  RequireRejected([](Response& response) { response.set_success(false); },
                  "failed response must be rejected");
  RequireRejected([](Response& response) { response.set_schema_major(1); },
                  "generic schema major must be rejected");
  RequireRejected([](Response& response) { response.set_schema_source_sha256("bad"); },
                  "malformed schema hash must be rejected");
  RequireRejected([](Response& response) { response.clear_contract_revision(); },
                  "missing contract revision must be rejected");
  RequireRejected([](Response& response) { response.set_board_id("016"); },
                  "wrong board must be rejected");
  RequireRejected([](Response& response) { response.set_request_sequence(43); },
                  "wrong request sequence must be rejected");
  RequireRejected([](Response& response) { response.clear_telemetry(); },
                  "missing BoardTelemetry must be rejected");
  RequireRejected([](Response& response) { response.mutable_telemetry()->clear_firmware_loaded(); },
                  "missing named scalar field must be rejected");
  RequireRejected(
      [](Response& response) { response.mutable_telemetry()->clear_afe_blocks_attenuation(); },
      "empty indexed field must be rejected");
  RequireRejected(
      [](Response& response) {
        response.mutable_telemetry()->mutable_afe_blocks_attenuation(0)->clear_afe();
      },
      "empty instance key must be rejected");
  RequireRejected(
      [](Response& response) {
        const auto& original = response.telemetry().afe_blocks_attenuation(0);
        *response.mutable_telemetry()->add_afe_blocks_attenuation() = original;
      },
      "duplicate rendered NodeId must be rejected");
  RequireRejected(
      [](Response& response) {
        ClearSampleValue(response.mutable_telemetry()->mutable_firmware_loaded());
      },
      "Good named field without a value must be rejected");
  RequireRejected(
      [](Response& response) {
        response.mutable_telemetry()->mutable_firmware_loaded()->mutable_metadata()->set_quality(
            daphne::telemetry::v8::TELEMETRY_QUALITY_UNAVAILABLE);
      },
      "Unavailable named field carrying a value must be rejected");
  RequireRejected(
      [](Response& response) {
        response.mutable_telemetry()->mutable_firmware_loaded()->mutable_metadata()->set_quality(
            daphne::telemetry::v8::TELEMETRY_QUALITY_UNSPECIFIED);
      },
      "unspecified quality must be rejected");
  RequireRejected(
      [](Response& response) {
        auto* load = response.mutable_telemetry()->mutable_host_cpu_load1_minute();
        load->mutable_metadata()->set_quality(daphne::telemetry::v8::TELEMETRY_QUALITY_GOOD);
        load->set_value(std::numeric_limits<double>::infinity());
      },
      "non-finite explicit double must be rejected");

  std::cout << "v8 explicit snapshot validation tests passed\n";
  return 0;
}
