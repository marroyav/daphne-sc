#include "v8_snapshot.hpp"

#include <google/protobuf/descriptor.h>

#include <cmath>
#include <set>
#include <stdexcept>
#include <utility>
#include <vector>

#ifndef PDS_DAPHNE_V8_SCHEMA_SHA256
#error "PDS_DAPHNE_V8_SCHEMA_SHA256 must identify the compiled telemetry schema"
#endif

namespace pds::bridge::v8 {
namespace {

using daphne::telemetry::v8::TELEMETRY_QUALITY_GOOD;
using daphne::telemetry::v8::TELEMETRY_QUALITY_INVALID;
using daphne::telemetry::v8::TELEMETRY_QUALITY_NOT_APPLICABLE;
using daphne::telemetry::v8::TELEMETRY_QUALITY_STALE;
using daphne::telemetry::v8::TELEMETRY_QUALITY_UNAVAILABLE;
using google::protobuf::FieldDescriptor;
using google::protobuf::Message;

constexpr int kExpectedDeclaredWireFields = 316;

template <typename DescriptorType>
std::string DescriptorName(const DescriptorType* descriptor) {
  return std::string(descriptor->full_name());
}

std::string BoardPrefix(const std::string& board_id) { return "DAPHNE.Boards." + board_id + "."; }

void ReplaceAll(std::string* value, const std::string& token, const std::string& replacement) {
  size_t offset = 0;
  while ((offset = value->find(token, offset)) != std::string::npos) {
    value->replace(offset, token.size(), replacement);
    offset += replacement.size();
  }
}

std::vector<std::string> ExtractPlaceholders(const std::string& pattern) {
  std::vector<std::string> placeholders;
  size_t offset = 0;
  while ((offset = pattern.find('{', offset)) != std::string::npos) {
    const size_t close = pattern.find('}', offset + 1);
    if (close == std::string::npos) {
      throw std::runtime_error("unterminated protobuf NodeId placeholder: " + pattern);
    }
    placeholders.push_back(pattern.substr(offset + 1, close - offset - 1));
    offset = close + 1;
  }
  return placeholders;
}

WireValueType ValueTypeForSample(const Message& sample) {
  const auto* descriptor = sample.GetDescriptor();
  if (descriptor == daphne::telemetry::v8::BooleanSample::descriptor()) {
    return WireValueType::Boolean;
  }
  if (descriptor == daphne::telemetry::v8::IntegerSample::descriptor()) {
    return WireValueType::Integer;
  }
  if (descriptor == daphne::telemetry::v8::LongSample::descriptor()) {
    return WireValueType::Long;
  }
  if (descriptor == daphne::telemetry::v8::DoubleSample::descriptor()) {
    return WireValueType::Double;
  }
  if (descriptor == daphne::telemetry::v8::StringSample::descriptor()) {
    return WireValueType::String;
  }
  if (descriptor == daphne::telemetry::v8::DateTimeSample::descriptor()) {
    return WireValueType::DateTime;
  }
  throw std::runtime_error("BoardTelemetry field has unsupported sample type: " +
                           DescriptorName(descriptor));
}

WireSampleView MakeSampleView(std::string node_id, const Message& sample) {
  const FieldDescriptor* value_field = sample.GetDescriptor()->FindFieldByName("value");
  const FieldDescriptor* metadata_field = sample.GetDescriptor()->FindFieldByName("metadata");
  if (value_field == nullptr || metadata_field == nullptr ||
      metadata_field->cpp_type() != FieldDescriptor::CPPTYPE_MESSAGE) {
    throw std::runtime_error("typed protobuf sample lacks value or metadata: " + node_id);
  }
  if (!sample.GetReflection()->HasField(sample, metadata_field)) {
    throw std::runtime_error("typed protobuf sample lacks SampleMetadata: " + node_id);
  }
  const Message& metadata_message = sample.GetReflection()->GetMessage(sample, metadata_field);
  const auto* metadata =
      dynamic_cast<const daphne::telemetry::v8::SampleMetadata*>(&metadata_message);
  if (metadata == nullptr) {
    throw std::runtime_error("typed protobuf sample has wrong metadata type: " + node_id);
  }
  return WireSampleView{std::move(node_id), ValueTypeForSample(sample), &sample, value_field,
                        metadata};
}

std::string RenderIndexedNodeId(const std::string& pattern, const Message& entry,
                                const std::vector<std::string>& placeholders) {
  std::string node_id = pattern;
  const auto* descriptor = entry.GetDescriptor();
  const auto* reflection = entry.GetReflection();
  const FieldDescriptor* sample_field = descriptor->FindFieldByName("sample");
  if (sample_field == nullptr || sample_field->cpp_type() != FieldDescriptor::CPPTYPE_MESSAGE ||
      descriptor->field_count() != static_cast<int>(placeholders.size()) + 1) {
    throw std::runtime_error("indexed protobuf wrapper has an invalid declared shape: " +
                             DescriptorName(descriptor));
  }
  for (size_t index = 0; index < placeholders.size(); ++index) {
    const std::string& placeholder = placeholders[index];
    const FieldDescriptor* key_field = descriptor->field(static_cast<int>(index));
    if (key_field == sample_field || key_field->cpp_type() != FieldDescriptor::CPPTYPE_STRING) {
      throw std::runtime_error("indexed protobuf key is not a declared string for {" + placeholder +
                               "}: " + DescriptorName(descriptor));
    }
    const std::string key = reflection->GetString(entry, key_field);
    if (key.empty()) {
      throw std::runtime_error("indexed protobuf wrapper has empty key for {" + placeholder +
                               "}: " + DescriptorName(descriptor));
    }
    ReplaceAll(&node_id, "{" + placeholder + "}", key);
  }
  if (node_id.find('{') != std::string::npos || node_id.find('}') != std::string::npos) {
    throw std::runtime_error("unresolved protobuf NodeId placeholder: " + node_id);
  }
  return node_id;
}

void ValidateSample(const WireSampleView& view, ValidatedSnapshot* result) {
  ++result->sample_count;
  const bool has_value = view.HasValue();
  const auto quality = view.metadata->quality();
  if ((quality == TELEMETRY_QUALITY_GOOD || quality == TELEMETRY_QUALITY_STALE) && !has_value) {
    throw std::runtime_error("telemetry sample lacks value: " + view.node_id);
  }
  if ((quality == TELEMETRY_QUALITY_UNAVAILABLE || quality == TELEMETRY_QUALITY_NOT_APPLICABLE) &&
      has_value) {
    throw std::runtime_error("unavailable telemetry sample carries a value: " + view.node_id);
  }
  if (view.value_type == WireValueType::Double && has_value &&
      !std::isfinite(view.sample->GetReflection()->GetDouble(*view.sample, view.value_field))) {
    throw std::runtime_error("telemetry sample carries a non-finite double: " + view.node_id);
  }

  switch (quality) {
    case TELEMETRY_QUALITY_GOOD:
      ++result->good_count;
      break;
    case TELEMETRY_QUALITY_STALE:
    case TELEMETRY_QUALITY_NOT_APPLICABLE:
      break;
    case TELEMETRY_QUALITY_UNAVAILABLE:
      ++result->unavailable_count;
      break;
    case TELEMETRY_QUALITY_INVALID:
      ++result->invalid_count;
      break;
    case daphne::telemetry::v8::TELEMETRY_QUALITY_UNSPECIFIED:
    default:
      throw std::runtime_error("unsupported telemetry quality: " + view.node_id);
  }
}

}  // namespace

bool WireSampleView::HasValue() const {
  return sample != nullptr && value_field != nullptr &&
         sample->GetReflection()->HasField(*sample, value_field);
}

void VisitWireSamples(const daphne::telemetry::v8::BoardTelemetry& telemetry,
                      const std::string& board_id, const WireSampleVisitor& visitor) {
  const auto* descriptor = telemetry.GetDescriptor();
  const auto* reflection = telemetry.GetReflection();
  if (descriptor->field_count() != kExpectedDeclaredWireFields) {
    throw std::runtime_error("BoardTelemetry declares " +
                             std::to_string(descriptor->field_count()) + " fields; expected " +
                             std::to_string(kExpectedDeclaredWireFields));
  }

  for (int field_index = 0; field_index < descriptor->field_count(); ++field_index) {
    const FieldDescriptor* field = descriptor->field(field_index);
    const auto& options = field->options();
    if (!options.HasExtension(daphne::telemetry::v8::opcua_node_pattern) ||
        !options.HasExtension(daphne::telemetry::v8::engineering_unit) ||
        !options.HasExtension(daphne::telemetry::v8::data_source) ||
        !options.HasExtension(daphne::telemetry::v8::control_owner)) {
      throw std::runtime_error("BoardTelemetry field lacks traceability annotations: " +
                               DescriptorName(field));
    }
    if (options.GetExtension(daphne::telemetry::v8::data_source).empty() ||
        options.GetExtension(daphne::telemetry::v8::control_owner).empty()) {
      throw std::runtime_error("BoardTelemetry field has empty source or owner: " +
                               DescriptorName(field));
    }

    std::string pattern = options.GetExtension(daphne::telemetry::v8::opcua_node_pattern);
    ReplaceAll(&pattern, "{BoardId}", board_id);
    const std::vector<std::string> placeholders = ExtractPlaceholders(pattern);
    if (!field->is_repeated()) {
      if (!placeholders.empty()) {
        throw std::runtime_error("scalar BoardTelemetry field contains an instance placeholder: " +
                                 DescriptorName(field));
      }
      if (!reflection->HasField(telemetry, field)) {
        throw std::runtime_error("explicit BoardTelemetry field is absent: " +
                                 DescriptorName(field));
      }
      visitor(MakeSampleView(pattern, reflection->GetMessage(telemetry, field)));
      continue;
    }

    const int entry_count = reflection->FieldSize(telemetry, field);
    if (placeholders.empty() || entry_count == 0) {
      throw std::runtime_error("indexed BoardTelemetry field has no key or instances: " +
                               DescriptorName(field));
    }
    for (int entry_index = 0; entry_index < entry_count; ++entry_index) {
      const Message& entry = reflection->GetRepeatedMessage(telemetry, field, entry_index);
      const FieldDescriptor* sample_field = entry.GetDescriptor()->FindFieldByName("sample");
      if (sample_field == nullptr || sample_field->cpp_type() != FieldDescriptor::CPPTYPE_MESSAGE ||
          !entry.GetReflection()->HasField(entry, sample_field)) {
        throw std::runtime_error("indexed BoardTelemetry entry lacks typed sample: " +
                                 DescriptorName(entry.GetDescriptor()));
      }
      visitor(MakeSampleView(RenderIndexedNodeId(pattern, entry, placeholders),
                             entry.GetReflection()->GetMessage(entry, sample_field)));
    }
  }
}

daphne::telemetry::v8::ReadTelemetrySnapshotRequest MakeRequest(uint64_t sequence) {
  daphne::telemetry::v8::ReadTelemetrySnapshotRequest request;
  request.set_request_sequence(sequence);
  return request;
}

const char* ExpectedSchemaSourceSha256() { return PDS_DAPHNE_V8_SCHEMA_SHA256; }

ValidatedSnapshot ParseAndValidate(const std::string& payload, const std::string& board_id,
                                   uint64_t request_sequence) {
  ValidatedSnapshot result;
  auto& response = result.response;
  if (!response.ParseFromString(payload)) {
    throw std::runtime_error("bad ReadTelemetrySnapshotResponse payload");
  }
  if (!response.success()) {
    throw std::runtime_error("telemetry response failed: " + response.message());
  }
  if (response.schema_major() != 2) {
    throw std::runtime_error("unsupported explicit telemetry schema major " +
                             std::to_string(response.schema_major()));
  }
  if (response.schema_source_sha256() != ExpectedSchemaSourceSha256()) {
    throw std::runtime_error("telemetry schema hash differs from the schema compiled by bridge");
  }
  if (response.contract_revision().empty()) {
    throw std::runtime_error("telemetry response has no contract revision");
  }
  if (response.board_id() != board_id) {
    throw std::runtime_error("telemetry board_id mismatch: expected " + board_id + ", got " +
                             response.board_id());
  }
  if (response.request_sequence() != request_sequence) {
    throw std::runtime_error("telemetry request_sequence mismatch");
  }
  if (!response.has_telemetry()) {
    throw std::runtime_error("telemetry response lacks explicit BoardTelemetry");
  }

  std::set<std::string> node_ids;
  const std::string prefix = BoardPrefix(board_id);
  VisitWireSamples(response.telemetry(), board_id, [&](const WireSampleView& view) {
    if (view.node_id.rfind(prefix, 0) != 0) {
      throw std::runtime_error("telemetry sample is outside configured board: " + view.node_id);
    }
    if (!node_ids.insert(view.node_id).second) {
      throw std::runtime_error("duplicate telemetry NodeId: " + view.node_id);
    }
    ValidateSample(view, &result);
  });
  if (node_ids.empty()) {
    throw std::runtime_error("explicit telemetry response contains no samples");
  }
  return result;
}

}  // namespace pds::bridge::v8
