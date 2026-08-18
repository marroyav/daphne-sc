#include "daphne_client.hpp"

#include <google/protobuf/util/json_util.h>
#include <unistd.h>

#include <array>
#include <chrono>
#include <ctime>
#include <iomanip>
#include <sstream>
#include <stdexcept>
#include <utility>
#include <zmq.hpp>

#include "daphneV3_high_level_confs.pb.h"
#include "daphneV3_low_level_confs.pb.h"
#include "v8_snapshot.hpp"

namespace pds::bridge {
namespace {

std::string Trim(const std::string& value) {
  const auto begin = value.find_first_not_of(" \t\r\n");
  if (begin == std::string::npos) return "";
  const auto end = value.find_last_not_of(" \t\r\n");
  return value.substr(begin, end - begin + 1);
}

uint64_t NowNs() {
  const auto now = std::chrono::system_clock::now().time_since_epoch();
  return static_cast<uint64_t>(std::chrono::duration_cast<std::chrono::nanoseconds>(now).count());
}

std::string NowIso8601() {
  const auto now = std::chrono::system_clock::now();
  const auto secs = std::chrono::time_point_cast<std::chrono::seconds>(now);
  const auto ms = std::chrono::duration_cast<std::chrono::milliseconds>(now - secs).count();
  const std::time_t time = std::chrono::system_clock::to_time_t(now);
  std::tm utc{};
  gmtime_r(&time, &utc);
  std::ostringstream output;
  output << std::put_time(&utc, "%Y-%m-%dT%H:%M:%S") << "." << std::setw(3) << std::setfill('0')
         << ms << "Z";
  return output.str();
}

}  // namespace

class DaphneClient::Impl {
 public:
  Impl(zmq::context_t& context, bool enabled, bool fake, std::string board_id, std::string endpoint,
       std::string route, int timeout_ms)
      : enabled_(enabled),
        fake_(fake),
        board_id_(std::move(board_id)),
        endpoint_(std::move(endpoint)),
        route_(std::move(route)),
        timeout_ms_(timeout_ms),
        socket_(context, zmq::socket_type::dealer) {
    const int linger = 0;
    const int receive_timeout = timeout_ms_;
    const int send_timeout = timeout_ms_;
    std::ostringstream identity_stream;
    identity_stream << "pds-opcua-bridge-" << getpid() << "-" << NowNs();
    const std::string identity = identity_stream.str();
#if defined(CPPZMQ_VERSION) && defined(ZMQ_MAKE_VERSION) && \
    CPPZMQ_VERSION >= ZMQ_MAKE_VERSION(4, 7, 0)
    socket_.set(zmq::sockopt::linger, linger);
    socket_.set(zmq::sockopt::rcvtimeo, receive_timeout);
    socket_.set(zmq::sockopt::sndtimeo, send_timeout);
    socket_.set(zmq::sockopt::routing_id, identity);
#else
    socket_.setsockopt(ZMQ_LINGER, &linger, sizeof(linger));
    socket_.setsockopt(ZMQ_RCVTIMEO, &receive_timeout, sizeof(receive_timeout));
    socket_.setsockopt(ZMQ_SNDTIMEO, &send_timeout, sizeof(send_timeout));
    socket_.setsockopt(ZMQ_IDENTITY, identity.data(), identity.size());
#endif
    if (enabled_ && !fake_) socket_.connect(endpoint_);
  }

  DaphneStatus Poll() {
    if (!enabled_) {
      DaphneStatus status;
      status.enabled = false;
      status.message = "disabled";
      status.lastUpdate = NowIso8601();
      status.updateTimeNs = NowNs();
      return status;
    }
    if (fake_) return FakeStatus();

    try {
      return PollTelemetryV8();
    } catch (const std::exception& err) {
      return PollLegacy(err.what());
    }
  }

  std::string Execute(const std::string& operation, const std::string& request_json) {
    if (!enabled_) throw std::runtime_error("DAPHNE backend is disabled");
    if (operation == "ConfigureRun" || operation == "ConfigureFrontend") {
      return TransactJson<daphne::ConfigureRequest, daphne::ConfigureResponse>(
          operation, request_json, daphne::MT2_CONFIGURE_FE_REQ, daphne::MT2_CONFIGURE_FE_RESP);
    }
    if (operation == "ConfigureClocks") {
      return TransactJson<daphne::ConfigureCLKsRequest, daphne::ConfigureCLKsResponse>(
          operation, request_json, daphne::MT2_CONFIGURE_CLKS_REQ, daphne::MT2_CONFIGURE_CLKS_RESP);
    }
    if (operation == "SetControlledBias") {
      return TransactJson<daphne::cmd_writeVbiasControl, daphne::cmd_writeVbiasControl_response>(
          operation, request_json, daphne::MT2_WRITE_VBIAS_CONTROL_REQ,
          daphne::MT2_WRITE_VBIAS_CONTROL_RESP);
    }
    if (operation == "SetAfePowerState") {
      return TransactJson<daphne::cmd_setAFEPowerState, daphne::cmd_setAFEPowerState_response>(
          operation, request_json, daphne::MT2_SET_AFE_POWERSTATE_REQ,
          daphne::MT2_SET_AFE_POWERSTATE_RESP);
    }
    if (operation == "ResetAfe") {
      return TransactJson<daphne::cmd_doAFEReset, daphne::cmd_doAFEReset_response>(
          operation, request_json, daphne::MT2_DO_AFE_RESET_REQ, daphne::MT2_DO_AFE_RESET_RESP);
    }
    if (operation == "AlignAfes") {
      return TransactJson<daphne::cmd_alignAFEs, daphne::cmd_alignAFEs_response>(
          operation, request_json, daphne::MT2_ALIGN_AFE_REQ, daphne::MT2_ALIGN_AFE_RESP);
    }
    if (operation == "SoftwareTrigger") {
      return TransactJson<daphne::cmd_doSoftwareTrigger, daphne::cmd_doSoftwareTrigger_response>(
          operation, request_json, daphne::MT2_DO_SOFTWARE_TRIGGER_REQ,
          daphne::MT2_DO_SOFTWARE_TRIGGER_RESP);
    }
    if (operation == "DumpSpyBuffers") {
      return TransactJson<daphne::DumpSpyBuffersRequest, daphne::DumpSpyBuffersResponse>(
          operation, request_json, daphne::MT2_DUMP_SPYBUFFER_REQ, daphne::MT2_DUMP_SPYBUFFER_RESP);
    }
    if (operation == "WriteAfeRegister") {
      return TransactJson<daphne::cmd_writeAFEReg, daphne::cmd_writeAFEReg_response>(
          operation, request_json, daphne::MT2_WRITE_AFE_REG_REQ, daphne::MT2_WRITE_AFE_REG_RESP);
    }
    throw std::runtime_error("no DAPHNE backend adapter for operation " + operation);
  }

 private:
  DaphneStatus PollLegacy(const std::string& telemetry_error) {
    DaphneStatus status;
    status.enabled = enabled_;
    status.lastUpdate = NowIso8601();
    status.updateTimeNs = NowNs();

    try {
      daphne::TestRegRequest test_request;
      const auto test_envelope =
          SendRequest(test_request, daphne::MT2_READ_TEST_REG_REQ, daphne::MT2_READ_TEST_REG_RESP);
      daphne::TestRegResponse test_response;
      if (!test_response.ParseFromString(test_envelope.payload())) {
        status.message = "bad TestRegResponse payload";
        return status;
      }
      if (test_response.value() != 0xDEADBEEFULL) {
        status.connected = true;
        status.testRegValue = test_response.value();
        status.testRegHex = Hex64(test_response.value());
        status.message = "unexpected test register value " + status.testRegHex;
        return status;
      }

      daphne::InfoRequest info_request;
      info_request.set_level(0);
      const auto info_envelope = SendRequest(info_request, daphne::MT2_READ_GENERAL_INFO_REQ,
                                             daphne::MT2_READ_GENERAL_INFO_RESP);
      daphne::GeneralInfo info;
      if (!info.ParseFromString(info_envelope.payload())) {
        status.connected = true;
        status.message = "bad GeneralInfo payload";
        return status;
      }

      status.connected = true;
      status.success = true;
      status.testRegValue = test_response.value();
      status.testRegHex = Hex64(test_response.value());
      status.message = "legacy daphneServer V2 readback ok; v8 unavailable: " + telemetry_error;
      status.firmwareBuildId = "legacy-daphneServer-v2";
      status.vBias0 = info.v_bias_0();
      status.vBias1 = info.v_bias_1();
      status.vBias2 = info.v_bias_2();
      status.vBias3 = info.v_bias_3();
      status.vBias4 = info.v_bias_4();
      status.powerMinus5V = info.power_minus5v();
      status.powerPlus2p5V = info.power_plus2p5v();
      status.powerCeV = info.power_ce();
      status.temperatureC = info.temperature();
      status.railCount = 3;
      status.temperatureCount = 1;
      status.errorCount = 0;
      status.generalInfo = GeneralInfoJson(info);
      status.rails = RailsJson(info);
      status.errors = "[]";
    } catch (const std::exception& err) {
      status.message =
          std::string("DAPHNE poll failed: v8=") + telemetry_error + "; legacy=" + err.what();
    }
    return status;
  }
  DaphneStatus PollTelemetryV8() {
    const auto request = pds::bridge::v8::MakeRequest(next_telemetry_sequence_++);
    const auto envelope = SendRequest(request, daphne::MT2_READ_TELEMETRY_SNAPSHOT_REQ,
                                      daphne::MT2_READ_TELEMETRY_SNAPSHOT_RESP);
    auto snapshot = pds::bridge::v8::ParseAndValidate(envelope.payload(), board_id_,
                                                      request.request_sequence());

    DaphneStatus status;
    status.enabled = enabled_;
    status.connected = true;
    status.success = true;
    status.telemetryV8 = true;
    status.lastUpdate = NowIso8601();
    status.updateTimeNs = snapshot.response.snapshot_time_unix_ns() != 0
                              ? snapshot.response.snapshot_time_unix_ns()
                              : NowNs();
    status.errorCount = static_cast<int>(snapshot.invalid_count);
    std::ostringstream message;
    message << "v8 telemetry ok: points=" << snapshot.response.points_size()
            << " good=" << snapshot.good_count << " unavailable=" << snapshot.unavailable_count
            << " invalid=" << snapshot.invalid_count;
    status.message = message.str();

    auto find_point =
        [&](const std::string& suffix) -> const daphne::telemetry::v8::TelemetryPoint* {
      return snapshot.GoodPoint(suffix);
    };
    if (const auto* point = find_point("Firmware.Loaded");
        point && point->value_case() == daphne::telemetry::v8::TelemetryPoint::kBooleanValue)
      status.firmwareLoaded = point->boolean_value();
    if (const auto* point = find_point("Firmware.BuildId");
        point && point->value_case() == daphne::telemetry::v8::TelemetryPoint::kStringValue)
      status.firmwareBuildId = point->string_value();
    if (const auto* point = find_point("Timing.Mmcm0Locked");
        point && point->value_case() == daphne::telemetry::v8::TelemetryPoint::kBooleanValue)
      status.mmcm0Locked = point->boolean_value();
    if (const auto* point = find_point("Timing.Mmcm1Locked");
        point && point->value_case() == daphne::telemetry::v8::TelemetryPoint::kBooleanValue)
      status.mmcm1Locked = point->boolean_value();
    const std::array<double DaphneStatus::*, 5> bias_members{{
        &DaphneStatus::vBias0,
        &DaphneStatus::vBias1,
        &DaphneStatus::vBias2,
        &DaphneStatus::vBias3,
        &DaphneStatus::vBias4,
    }};
    for (size_t afe = 0; afe < bias_members.size(); ++afe) {
      if (const auto* point = find_point("AFE.Blocks." + std::to_string(afe) + ".BiasVoltage");
          point && point->value_case() == daphne::telemetry::v8::TelemetryPoint::kDoubleValue)
        status.*bias_members[afe] = point->double_value();
    }
    status.telemetry = std::move(snapshot.response);
    return status;
  }

  template <typename Request, typename Response>
  std::string TransactJson(const std::string& operation, const std::string& request_json,
                           daphne::MessageTypeV2 request_type,
                           daphne::MessageTypeV2 response_type) {
    Request request;
    const std::string json = Trim(request_json).empty() ? "{}" : request_json;
    const auto parse_status = google::protobuf::util::JsonStringToMessage(json, &request);
    if (!parse_status.ok())
      throw std::runtime_error("invalid " + operation + " JSON: " + parse_status.ToString());
    if (fake_) return "{\"success\":true,\"message\":\"fake " + operation + " accepted\"}";

    const auto response_envelope = SendRequest(request, request_type, response_type);
    Response response;
    if (!response.ParseFromString(response_envelope.payload()))
      throw std::runtime_error("bad " + operation + " response payload");
    std::string response_json;
    const auto print_status = google::protobuf::util::MessageToJsonString(response, &response_json);
    if (!print_status.ok())
      throw std::runtime_error("cannot encode " + operation +
                               " response: " + print_status.ToString());
    return response_json;
  }

  DaphneStatus FakeStatus() const {
    DaphneStatus status;
    status.enabled = enabled_;
    status.connected = true;
    status.success = true;
    status.firmwareLoaded = true;
    status.rpuAvailable = false;
    status.rpuRunning = false;
    status.mmcm0Locked = true;
    status.mmcm1Locked = true;
    status.testRegValue = 0xDEADBEEFULL;
    status.testRegHex = "0x00000000DEADBEEF";
    status.vBias0 = 0.63150;
    status.vBias1 = 0.45927;
    status.vBias2 = 0.0;
    status.vBias3 = 0.00594;
    status.vBias4 = 0.01386;
    status.powerMinus5V = -5.02533;
    status.powerPlus2p5V = 3.30020;
    status.powerCeV = 1.80146;
    status.temperatureC = 0.0;
    status.railCount = 3;
    status.temperatureCount = 1;
    status.errorCount = 0;
    status.message = "fake legacy daphneServer V2 readback";
    status.firmwareBuildId = "fake-legacy-daphneServer-v2";
    status.generalInfo = GeneralInfoJson(status);
    status.rails = RailsJson(status);
    status.errors = "[]";
    status.lastUpdate = NowIso8601();
    status.updateTimeNs = NowNs();
    return status;
  }

  template <typename Request>
  daphne::ControlEnvelopeV2 SendRequest(const Request& request, daphne::MessageTypeV2 request_type,
                                        daphne::MessageTypeV2 response_type) {
    std::string request_payload;
    request.SerializeToString(&request_payload);

    daphne::ControlEnvelopeV2 envelope;
    envelope.set_version(2);
    envelope.set_dir(daphne::DIR_REQUEST);
    envelope.set_type(request_type);
    envelope.set_payload(request_payload);
    envelope.set_task_id(next_message_id_);
    envelope.set_msg_id(next_message_id_++);
    envelope.set_route(route_);
    envelope.set_timestamp_ns(NowNs());

    std::string bytes;
    envelope.SerializeToString(&bytes);
    if (!socket_.send(zmq::buffer(bytes), zmq::send_flags::none))
      throw std::runtime_error("ZMQ send timed out");

    // A late response can remain queued after a timeout. Discard stale
    // envelopes until the response correlated to this request arrives,
    // while keeping one overall timeout budget for the operation.
    const auto deadline = std::chrono::steady_clock::now() + std::chrono::milliseconds(timeout_ms_);
    for (;;) {
      const auto remaining = std::chrono::duration_cast<std::chrono::milliseconds>(
          deadline - std::chrono::steady_clock::now());
      if (remaining.count() <= 0) throw std::runtime_error("ZMQ receive timed out");
#if defined(CPPZMQ_VERSION) && defined(ZMQ_MAKE_VERSION) && \
    CPPZMQ_VERSION >= ZMQ_MAKE_VERSION(4, 7, 0)
      socket_.set(zmq::sockopt::rcvtimeo, static_cast<int>(remaining.count()));
#else
      const int receive_timeout = static_cast<int>(remaining.count());
      socket_.setsockopt(ZMQ_RCVTIMEO, &receive_timeout, sizeof(receive_timeout));
#endif

      zmq::message_t response_bytes;
      const auto received = socket_.recv(response_bytes, zmq::recv_flags::none);
      if (!received) throw std::runtime_error("ZMQ receive timed out");

      daphne::ControlEnvelopeV2 response_envelope;
      if (!response_envelope.ParseFromArray(response_bytes.data(),
                                            static_cast<int>(response_bytes.size())))
        throw std::runtime_error("bad ControlEnvelopeV2 response");
      if (response_envelope.correl_id() != 0 && response_envelope.correl_id() != envelope.msg_id())
        continue;
      if (response_envelope.type() != response_type) {
        throw std::runtime_error("unexpected DAPHNE response type " +
                                 std::to_string(response_envelope.type()));
      }
      return response_envelope;
    }
  }

  static std::string Hex64(uint64_t value) {
    std::ostringstream out;
    out << "0x" << std::uppercase << std::hex << std::setw(16) << std::setfill('0') << value;
    return out.str();
  }

  static std::string GeneralInfoJson(const daphne::GeneralInfo& info) {
    std::ostringstream out;
    out << "{\"v_bias_0\":" << info.v_bias_0() << ",\"v_bias_1\":" << info.v_bias_1()
        << ",\"v_bias_2\":" << info.v_bias_2() << ",\"v_bias_3\":" << info.v_bias_3()
        << ",\"v_bias_4\":" << info.v_bias_4() << ",\"power_minus5v\":" << info.power_minus5v()
        << ",\"power_plus2p5v\":" << info.power_plus2p5v() << ",\"power_ce\":" << info.power_ce()
        << ",\"temperature\":" << info.temperature() << '}';
    return out.str();
  }

  static std::string GeneralInfoJson(const DaphneStatus& status) {
    std::ostringstream out;
    out << "{\"v_bias_0\":" << status.vBias0 << ",\"v_bias_1\":" << status.vBias1
        << ",\"v_bias_2\":" << status.vBias2 << ",\"v_bias_3\":" << status.vBias3
        << ",\"v_bias_4\":" << status.vBias4 << ",\"power_minus5v\":" << status.powerMinus5V
        << ",\"power_plus2p5v\":" << status.powerPlus2p5V << ",\"power_ce\":" << status.powerCeV
        << ",\"temperature\":" << status.temperatureC << '}';
    return out.str();
  }

  static std::string RailsJson(const daphne::GeneralInfo& info) {
    std::ostringstream out;
    out << "[{\"name\":\"-5V\",\"voltage_v\":" << info.power_minus5v()
        << ",\"status\":\"readback\"}"
        << ",{\"name\":\"+3.3V_PDS\",\"voltage_v\":" << info.power_plus2p5v()
        << ",\"status\":\"readback\"}"
        << ",{\"name\":\"+1.8V_A\",\"voltage_v\":" << info.power_ce()
        << ",\"status\":\"readback\"}]";
    return out.str();
  }

  static std::string RailsJson(const DaphneStatus& status) {
    std::ostringstream out;
    out << "[{\"name\":\"-5V\",\"voltage_v\":" << status.powerMinus5V << ",\"status\":\"readback\"}"
        << ",{\"name\":\"+3.3V_PDS\",\"voltage_v\":" << status.powerPlus2p5V
        << ",\"status\":\"readback\"}"
        << ",{\"name\":\"+1.8V_A\",\"voltage_v\":" << status.powerCeV
        << ",\"status\":\"readback\"}]";
    return out.str();
  }

  bool enabled_;
  bool fake_;
  std::string board_id_;
  std::string endpoint_;
  std::string route_;
  int timeout_ms_;
  zmq::socket_t socket_;
  uint64_t next_message_id_ = 1;
  uint64_t next_telemetry_sequence_ = 1;
};

DaphneClient::DaphneClient(zmq::context_t& context, bool enabled, bool fake, std::string board_id,
                           std::string endpoint, std::string route, int timeout_ms)
    : impl_(std::make_unique<Impl>(context, enabled, fake, std::move(board_id), std::move(endpoint),
                                   std::move(route), timeout_ms)) {}

DaphneClient::~DaphneClient() = default;

DaphneStatus DaphneClient::Poll() { return impl_->Poll(); }

std::string DaphneClient::Execute(const std::string& operation, const std::string& request_json) {
  return impl_->Execute(operation, request_json);
}

}  // namespace pds::bridge
