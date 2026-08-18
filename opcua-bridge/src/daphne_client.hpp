#ifndef DUNE_DAPHNE_SC_OPCUA_BRIDGE_SRC_DAPHNE_CLIENT_HPP_
#define DUNE_DAPHNE_SC_OPCUA_BRIDGE_SRC_DAPHNE_CLIENT_HPP_

#include <cstdint>
#include <memory>
#include <string>

#include "daphne_v8_telemetry.pb.h"

namespace zmq {
class context_t;
}

namespace pds::bridge {

// One polling result. The legacy scalar fields and their names are retained for
// compatibility with main.cpp; native v8 data is carried in telemetry and
// published point-for-point. A later, isolated legacy-server cleanup can rename
// these fields without obscuring the v8 transport change.
struct DaphneStatus {
  bool enabled = true;
  bool connected = false;
  bool success = false;
  bool firmwareLoaded = false;
  bool rpuAvailable = false;
  bool rpuRunning = false;
  bool mmcm0Locked = false;
  bool mmcm1Locked = false;
  uint64_t testRegValue = 0;
  double vBias0 = 0.0;
  double vBias1 = 0.0;
  double vBias2 = 0.0;
  double vBias3 = 0.0;
  double vBias4 = 0.0;
  double powerMinus5V = 0.0;
  double powerPlus2p5V = 0.0;
  double powerCeV = 0.0;
  double temperatureC = 0.0;
  int railCount = 0;
  int temperatureCount = 0;
  int errorCount = 0;
  std::string message = "not polled";
  std::string firmwareBuildId;
  std::string testRegHex = "0x00000000";
  std::string generalInfo = "{}";
  std::string rails = "[]";
  std::string errors = "[]";
  std::string lastUpdate;
  uint64_t updateTimeNs = 0;
  bool telemetryV8 = false;
  daphne::telemetry::v8::ReadTelemetrySnapshotResponse telemetry;
};

// ZMQ + ControlEnvelopeV2 client. Poll() tries the v8 snapshot first and uses
// the existing two-message legacy readback only when v8 is unavailable.
class DaphneClient {
 public:
  DaphneClient(zmq::context_t& context, bool enabled, bool fake, std::string board_id,
               std::string endpoint, std::string route, int timeout_ms);
  ~DaphneClient();

  DaphneClient(const DaphneClient&) = delete;
  DaphneClient& operator=(const DaphneClient&) = delete;

  DaphneStatus Poll();
  std::string Execute(const std::string& operation, const std::string& request_json);

 private:
  class Impl;
  std::unique_ptr<Impl> impl_;
};

}  // namespace pds::bridge

#endif  // DUNE_DAPHNE_SC_OPCUA_BRIDGE_SRC_DAPHNE_CLIENT_HPP_
