#include <open62541/server.h>
#include <open62541/server_config_default.h>
#include <open62541/plugin/accesscontrol_default.h>

#include <google/protobuf/message.h>
#include <zmq.hpp>

#include "daphne_client.hpp"
#include "opcua_snapshot_writer.hpp"
#include "registry.hpp"

#include <algorithm>
#include <atomic>
#include <chrono>
#include <csignal>
#include <cctype>
#include <cstdint>
#include <cstdlib>
#include <cstring>
#include <cerrno>
#include <fcntl.h>
#include <filesystem>
#include <fstream>
#include <glob.h>
#include <iomanip>
#include <iostream>
#include <map>
#include <memory>
#include <mutex>
#include <optional>
#include <set>
#include <sstream>
#include <stdexcept>
#include <string>
#include <sys/select.h>
#include <sys/stat.h>
#include <termios.h>
#include <thread>
#include <unistd.h>
#include <utility>
#include <vector>

namespace {

constexpr const char *kNamespaceUri = "urn:dune:pds:np04";
constexpr const char *kControlNamespaceUri = "urn:dune:pds:daphne";

std::atomic_bool g_running{true};

void handleSignal(int) { g_running.store(false); }

std::string trim(const std::string &value) {
    const auto begin = value.find_first_not_of(" \t\r\n");
    if(begin == std::string::npos)
        return "";
    const auto end = value.find_last_not_of(" \t\r\n");
    return value.substr(begin, end - begin + 1);
}

bool parseBool(const std::string &value) {
    std::string lower = value;
    std::transform(lower.begin(), lower.end(), lower.begin(), [](unsigned char c) {
        return static_cast<char>(std::tolower(c));
    });
    return lower == "1" || lower == "true" || lower == "yes" || lower == "on";
}

enum class ClientRole : uint32_t {
    Anonymous = 0,
    Daq = 1u << 0,
    SlowControls = 1u << 1,
    Dts = 1u << 2,
    Dps = 1u << 3,
    Expert = 1u << 4,
};

using RoleMask = uint32_t;

RoleMask roleMask(ClientRole role) {
    return static_cast<RoleMask>(role);
}

ClientRole parseRole(const std::string &value) {
    std::string role = trim(value);
    std::transform(role.begin(), role.end(), role.begin(), [](unsigned char c) {
        return static_cast<char>(std::tolower(c));
    });
    if(role == "daq") return ClientRole::Daq;
    if(role == "sc" || role == "slowcontrols" || role == "slow_controls")
        return ClientRole::SlowControls;
    if(role == "dts") return ClientRole::Dts;
    if(role == "dps") return ClientRole::Dps;
    if(role == "expert") return ClientRole::Expert;
    throw std::runtime_error("unknown OPC-UA role: " + value);
}

std::vector<std::string> parseCsvRecord(const std::string &line) {
    std::vector<std::string> fields;
    std::string field;
    bool quoted = false;
    for(size_t index = 0; index < line.size(); ++index) {
        const char ch = line[index];
        if(ch == '"') {
            if(quoted && index + 1 < line.size() && line[index + 1] == '"') {
                field.push_back('"');
                ++index;
            } else {
                quoted = !quoted;
            }
        } else if(ch == ',' && !quoted) {
            fields.push_back(field);
            field.clear();
        } else {
            field.push_back(ch);
        }
    }
    if(quoted)
        throw std::runtime_error("unterminated quoted CSV field");
    fields.push_back(field);
    return fields;
}

struct Credential {
    std::string username;
    std::string password;
    ClientRole role = ClientRole::Anonymous;
};

struct ControlPolicyEntry {
    std::string operation;
    std::string nodeIdPattern;
    std::string owner;
    RoleMask allowedRoles = 0;
    std::string implementationStatus;
    std::string backendMapping;
    bool executable = false;
};

uint64_t nowNs() {
    const auto now = std::chrono::system_clock::now().time_since_epoch();
    return static_cast<uint64_t>(
        std::chrono::duration_cast<std::chrono::nanoseconds>(now).count());
}

std::string nowIsoLike() {
    const auto now = std::chrono::system_clock::now();
    const auto secs = std::chrono::time_point_cast<std::chrono::seconds>(now);
    const auto ms = std::chrono::duration_cast<std::chrono::milliseconds>(now - secs).count();
    const std::time_t t = std::chrono::system_clock::to_time_t(now);
    std::tm tm{};
    gmtime_r(&t, &tm);
    std::ostringstream out;
    out << std::put_time(&tm, "%Y-%m-%dT%H:%M:%S") << "." << std::setw(3)
        << std::setfill('0') << ms << "Z";
    return out.str();
}

int parseEndpointPort(const std::string &endpoint) {
    const auto scheme = endpoint.find("://");
    const auto start = scheme == std::string::npos ? 0 : scheme + 3;
    const auto slash = endpoint.find('/', start);
    const auto hostPort = endpoint.substr(start, slash == std::string::npos ? std::string::npos : slash - start);
    const auto colon = hostPort.rfind(':');
    if(colon == std::string::npos)
        return 4840;
    const auto port = std::stoi(hostPort.substr(colon + 1));
    if(port < 1 || port > 65535)
        throw std::runtime_error("OPC-UA endpoint port out of range: " + std::to_string(port));
    return port;
}

std::vector<std::string> globPaths(const std::string &pattern) {
    glob_t result{};
    std::vector<std::string> paths;
    if(glob(pattern.c_str(), 0, nullptr, &result) == 0) {
        for(size_t i = 0; i < result.gl_pathc; ++i)
            paths.emplace_back(result.gl_pathv[i]);
    }
    globfree(&result);
    std::sort(paths.begin(), paths.end());
    return paths;
}

struct Config {
    std::string configPath;
    std::string opcuaEndpoint = "opc.tcp://0.0.0.0:4840";
    int pollPeriodMs = 1000;
    bool opcuaAllowAnonymousRead = true;
    std::string opcuaAuthFile;
    std::string controlPolicyPath;
    std::vector<Credential> credentials;
    std::vector<ControlPolicyEntry> controlPolicy;

    std::string registryTagListPath;
    std::string registryNamespaceVersion = "v8-draft-2026-08-17";
    int registryStaleAfterMs = 5000;
    size_t registryExpectedPatterns = 0;
    pds::registry::ExpansionConfig registryExpansion =
        pds::registry::defaultExpansionConfig();
    std::vector<pds::registry::Entry> registryEntries;
    std::vector<pds::registry::ExpandedNode> registryNodes;

    bool daphneEnabled = true;
    bool daphneFake = true;
    bool daphneWritesEnabled = false;
    std::string daphneEndpoint = "tcp://NP04-DAPHNE-015.CERN.CH:40001";
    std::string daphneRoute = "mezz/0";
    int daphneTimeoutMs = 750;
    int daphneWorkerCount = 16;
    std::vector<std::string> daphneIds;
    std::map<std::string, std::string> daphneEndpoints;
    std::map<std::string, std::string> daphneRoutes;

    bool powerEnabled = true;
    bool powerFake = true;
    std::string powerDevice = "auto";
    int powerTimeoutMs = 750;
    int powerSerialBaud = 9600;
    bool powerWritesEnabled = false;
    double powerVoltageMin = 0.0;
    double powerVoltageMax = 0.0;
    double powerCurrentMin = 0.0;
    double powerCurrentMax = 0.0;
};

std::vector<std::string> parseCsv(const std::string &value) {
    std::vector<std::string> values;
    std::istringstream input(value);
    std::string item;
    while(std::getline(input, item, ',')) {
        item = trim(item);
        if(!item.empty())
            values.push_back(item);
    }
    return values;
}

std::vector<Credential> loadCredentials(const std::string &path) {
    if(path.empty())
        return {};

    struct stat info {};
    if(stat(path.c_str(), &info) != 0)
        throw std::runtime_error("cannot stat OPC-UA auth file: " + path);
    if((info.st_mode & (S_IRWXG | S_IRWXO)) != 0)
        throw std::runtime_error("OPC-UA auth file must not be accessible by group or other: " + path);

    std::ifstream input(path);
    if(!input)
        throw std::runtime_error("cannot open OPC-UA auth file: " + path);
    std::vector<Credential> credentials;
    std::set<std::string> usernames;
    std::string line;
    size_t lineNo = 0;
    while(std::getline(input, line)) {
        ++lineNo;
        if(trim(line).empty() || trim(line).front() == '#')
            continue;
        const auto fields = parseCsvRecord(line);
        if(fields.size() != 3)
            throw std::runtime_error("auth file line " + std::to_string(lineNo) +
                                     " must contain username,password,role");
        Credential credential{trim(fields[0]), fields[1], parseRole(fields[2])};
        if(credential.username.empty() || credential.password.empty())
            throw std::runtime_error("empty username/password in auth file line " +
                                     std::to_string(lineNo));
        if(!usernames.insert(credential.username).second)
            throw std::runtime_error("duplicate OPC-UA username: " + credential.username);
        credentials.push_back(std::move(credential));
    }
    return credentials;
}

std::vector<ControlPolicyEntry> loadControlPolicy(const std::string &path) {
    if(path.empty())
        return {};
    std::ifstream input(path);
    if(!input)
        throw std::runtime_error("cannot open OPC-UA control policy: " + path);

    std::string line;
    if(!std::getline(input, line))
        throw std::runtime_error("empty OPC-UA control policy: " + path);
    const auto header = parseCsvRecord(line);
    std::map<std::string, size_t> columns;
    for(size_t index = 0; index < header.size(); ++index)
        columns[trim(header[index])] = index;
    const std::vector<std::string> required = {
        "Operation", "NodeId pattern", "Control owner", "Allowed roles",
        "Implementation status", "Backend mapping", "Executable",
    };
    for(const auto &name : required) {
        if(columns.find(name) == columns.end())
            throw std::runtime_error("OPC-UA control policy missing column: " + name);
    }

    std::vector<ControlPolicyEntry> policy;
    std::set<std::string> operations;
    size_t lineNo = 1;
    while(std::getline(input, line)) {
        ++lineNo;
        if(trim(line).empty())
            continue;
        const auto fields = parseCsvRecord(line);
        auto field = [&](const std::string &name) -> std::string {
            const size_t index = columns.at(name);
            if(index >= fields.size())
                throw std::runtime_error("short control-policy row " + std::to_string(lineNo));
            return trim(fields[index]);
        };
        ControlPolicyEntry entry;
        entry.operation = field("Operation");
        entry.nodeIdPattern = field("NodeId pattern");
        entry.owner = field("Control owner");
        entry.implementationStatus = field("Implementation status");
        entry.backendMapping = field("Backend mapping");
        entry.executable = parseBool(field("Executable"));
        for(const auto &role : parseCsv(field("Allowed roles")))
            entry.allowedRoles |= roleMask(parseRole(role));
        if(entry.operation.empty() || entry.nodeIdPattern.empty() || entry.allowedRoles == 0)
            throw std::runtime_error("incomplete control-policy row " + std::to_string(lineNo));
        if(entry.nodeIdPattern.find("{BoardId}") == std::string::npos)
            throw std::runtime_error("control-policy NodeId lacks {BoardId}: " + entry.nodeIdPattern);
        if(entry.executable && entry.backendMapping.empty())
            throw std::runtime_error("executable control-policy row lacks backend mapping: " + entry.operation);
        if(!operations.insert(entry.operation).second)
            throw std::runtime_error("duplicate control-policy operation: " + entry.operation);
        policy.push_back(std::move(entry));
    }
    return policy;
}

bool parseDaphneTargetValue(Config &config, const std::string &key, const std::string &value) {
    constexpr const char *prefix = "daphne.";
    if(key.rfind(prefix, 0) != 0)
        return false;

    const std::string suffix = key.substr(std::char_traits<char>::length(prefix));
    const auto separator = suffix.rfind('.');
    if(separator == std::string::npos)
        return false;

    const std::string id = suffix.substr(0, separator);
    const std::string property = suffix.substr(separator + 1);
    if(id.empty())
        return false;
    if(property == "endpoint") {
        config.daphneEndpoints[id] = value;
        return true;
    }
    if(property == "route") {
        config.daphneRoutes[id] = value;
        return true;
    }
    return false;
}

bool parseRegistryInstanceValue(Config &config, const std::string &key,
                                const std::string &value) {
    constexpr const char *prefix = "registry.instances.";
    if(key.rfind(prefix, 0) != 0)
        return false;
    const std::string placeholder =
        key.substr(std::char_traits<char>::length(prefix));
    if(placeholder.empty())
        throw std::runtime_error("empty registry instance placeholder");
    config.registryExpansion.instances[placeholder] =
        pds::registry::parseInstanceList(value);
    return true;
}

void applyConfigValue(Config &config, const std::string &key, const std::string &value) {
    if(key == "opcua.endpoint") config.opcuaEndpoint = value;
    else if(key == "opcua.allow_anonymous_read") config.opcuaAllowAnonymousRead = parseBool(value);
    else if(key == "opcua.auth_file") config.opcuaAuthFile = value;
    else if(key == "control.policy_file") config.controlPolicyPath = value;
    else if(key == "registry.tag_list_file") config.registryTagListPath = value;
    else if(key == "registry.namespace_version") config.registryNamespaceVersion = value;
    else if(key == "registry.stale_after_ms") config.registryStaleAfterMs = std::stoi(value);
    else if(key == "registry.expected_patterns")
        config.registryExpectedPatterns = static_cast<size_t>(std::stoull(value));
    else if(key == "registry.maximum_nodes")
        config.registryExpansion.maximumNodes = static_cast<size_t>(std::stoull(value));
    else if(key == "registry.i2c_devices")
        config.registryExpansion.i2cDevices = pds::registry::parseI2cDevices(value);
    else if(key == "poll.period_ms") config.pollPeriodMs = std::stoi(value);
    else if(key == "daphne.enabled") config.daphneEnabled = parseBool(value);
    else if(key == "daphne.fake") config.daphneFake = parseBool(value);
    else if(key == "daphne.writes_enabled") config.daphneWritesEnabled = parseBool(value);
    else if(key == "daphne.ids") config.daphneIds = parseCsv(value);
    else if(key == "daphne.endpoint") config.daphneEndpoint = value;
    else if(key == "daphne.route") config.daphneRoute = value;
    else if(key == "daphne.timeout_ms") config.daphneTimeoutMs = std::stoi(value);
    else if(key == "daphne.worker_count") config.daphneWorkerCount = std::stoi(value);
    else if(key == "power.enabled") config.powerEnabled = parseBool(value);
    else if(key == "power.fake") config.powerFake = parseBool(value);
    else if(key == "power.device") config.powerDevice = value;
    else if(key == "power.timeout_ms") config.powerTimeoutMs = std::stoi(value);
    else if(key == "power.serial_baud") config.powerSerialBaud = std::stoi(value);
    else if(key == "power.writes_enabled") config.powerWritesEnabled = parseBool(value);
    else if(key == "power.voltage_min_v") config.powerVoltageMin = std::stod(value);
    else if(key == "power.voltage_max_v") config.powerVoltageMax = std::stod(value);
    else if(key == "power.current_min_a") config.powerCurrentMin = std::stod(value);
    else if(key == "power.current_max_a") config.powerCurrentMax = std::stod(value);
    else if(parseDaphneTargetValue(config, key, value)) {}
    else if(parseRegistryInstanceValue(config, key, value)) {}
    else std::cerr << "warning: ignoring unknown config key: " << key << "\n";
}

Config loadConfig(const std::string &path) {
    Config config;
    config.configPath = path;
    if(path.empty()) {
        config.daphneIds.push_back("015");
        config.daphneEndpoints["015"] = config.daphneEndpoint;
        config.daphneRoutes["015"] = config.daphneRoute;
    } else {
        std::ifstream input(path);
        if(!input)
            throw std::runtime_error("cannot open config: " + path);

        std::string line;
        size_t lineNo = 0;
        while(std::getline(input, line)) {
            ++lineNo;
            const auto comment = line.find('#');
            if(comment != std::string::npos)
                line = line.substr(0, comment);
            line = trim(line);
            if(line.empty())
                continue;
            const auto eq = line.find('=');
            if(eq == std::string::npos)
                throw std::runtime_error("bad config line " + std::to_string(lineNo) + ": " + line);
            applyConfigValue(config, trim(line.substr(0, eq)), trim(line.substr(eq + 1)));
        }
    }

    if(config.pollPeriodMs < 100)
        throw std::runtime_error("poll.period_ms must be at least 100");
    if(config.daphneTimeoutMs < 1 || config.powerTimeoutMs < 1)
        throw std::runtime_error("timeouts must be positive");
    if(config.daphneWorkerCount < 1 || config.daphneWorkerCount > 256)
        throw std::runtime_error("daphne.worker_count must be between 1 and 256");
    if(config.registryStaleAfterMs < config.pollPeriodMs)
        throw std::runtime_error("registry.stale_after_ms must be at least poll.period_ms");
    if(config.registryExpansion.maximumNodes < 1)
        throw std::runtime_error("registry.maximum_nodes must be positive");
    if(config.daphneIds.empty())
        config.daphneIds.push_back("015");
    std::map<std::string, bool> seenIds;
    for(const auto &id : config.daphneIds) {
        if(id.empty() || id.find('.') != std::string::npos || id.find('/') != std::string::npos)
            throw std::runtime_error("invalid DAPHNE id: " + id);
        if(seenIds[id])
            throw std::runtime_error("duplicate DAPHNE id: " + id);
        seenIds[id] = true;
        if(config.daphneEndpoints.find(id) == config.daphneEndpoints.end())
            config.daphneEndpoints[id] = config.daphneEndpoint;
        if(config.daphneRoutes.find(id) == config.daphneRoutes.end())
            config.daphneRoutes[id] = config.daphneRoute;
    }
    if(const char *endpoint = std::getenv("PDS_OPCUA_ENDPOINT"))
        config.opcuaEndpoint = endpoint;
    if(const char *registry = std::getenv("PDS_OPCUA_REGISTRY"))
        config.registryTagListPath = registry;
    if(const char *policy = std::getenv("PDS_OPCUA_CONTROL_POLICY"))
        config.controlPolicyPath = policy;
    config.credentials = loadCredentials(config.opcuaAuthFile);
    config.controlPolicy = loadControlPolicy(config.controlPolicyPath);
    config.registryExpansion.instances["BoardId"] = config.daphneIds;
    if(!config.registryTagListPath.empty()) {
        config.registryEntries = pds::registry::loadTagList(config.registryTagListPath);
        config.registryNodes =
            pds::registry::expand(config.registryEntries, config.registryExpansion);
        const auto summary =
            pds::registry::summarize(config.registryEntries, config.registryNodes);
        if(config.registryExpectedPatterns != 0 &&
           summary.patterns != config.registryExpectedPatterns) {
            throw std::runtime_error(
                "registry pattern count mismatch: expected " +
                std::to_string(config.registryExpectedPatterns) + ", got " +
                std::to_string(summary.patterns));
        }
    }
    if(config.daphneWritesEnabled) {
        if(config.credentials.empty())
            throw std::runtime_error("daphne.writes_enabled requires opcua.auth_file");
        if(config.controlPolicy.empty())
            throw std::runtime_error("daphne.writes_enabled requires control.policy_file");
    }
    return config;
}

using pds::bridge::DaphneClient;
using pds::bridge::DaphneStatus;

struct PowerStatus {
    bool enabled = true;
    bool connected = false;
    bool outputEnabled = false;
    double voltageV = 0.0;
    double currentA = 0.0;
    std::string idn;
    std::string fault;
    std::string message = "not polled";
    std::string lastUpdate;
};

speed_t baudToTermios(int baud) {
    switch(baud) {
    case 1200: return B1200;
    case 2400: return B2400;
    case 4800: return B4800;
    case 9600: return B9600;
    case 19200: return B19200;
    case 38400: return B38400;
    case 57600: return B57600;
    case 115200: return B115200;
    default: return B9600;
    }
}

class ScpiPowerSupply {
  public:
    explicit ScpiPowerSupply(const Config &config) : config_(config) {}

    ~ScpiPowerSupply() {
        if(fd_ >= 0)
            close(fd_);
    }

    PowerStatus poll() {
        PowerStatus status;
        status.enabled = config_.powerEnabled;
        status.lastUpdate = nowIsoLike();
        if(!config_.powerEnabled) {
            status.message = "disabled";
            return status;
        }
        if(config_.powerFake)
            return fakeStatus();

        try {
            if(!ensureOpen(status))
                return status;
            status.connected = true;
            status.idn = query("*IDN?");
            status.outputEnabled = parseBoolish(query("OUTP?"));
            status.voltageV = parseFirstDouble(query("MEAS:VOLT?"));
            status.currentA = parseFirstDouble(query("MEAS:CURR?"));
            status.fault = query("SYST:ERR?");
            status.message = "ok";
        } catch(const std::exception &err) {
            status.connected = false;
            status.outputEnabled = false;
            status.message = std::string("power poll failed: ") + err.what();
            closeDevice();
        }
        return status;
    }

    std::string setOutput(bool enabled) {
        if(config_.powerFake)
            return enabled ? "fake output enabled" : "fake output disabled";
        requireWrites();
        command(std::string("OUTP ") + (enabled ? "1" : "0"));
        return enabled ? "output enabled" : "output disabled";
    }

    std::string setVoltage(double voltage) {
        if(config_.powerFake)
            return "fake voltage accepted";
        requireWrites();
        if(voltage < config_.powerVoltageMin || voltage > config_.powerVoltageMax)
            throw std::runtime_error("requested voltage outside configured bounds");
        command("VOLT " + toScpiNumber(voltage));
        return "voltage set";
    }

    std::string setCurrent(double current) {
        if(config_.powerFake)
            return "fake current accepted";
        requireWrites();
        if(current < config_.powerCurrentMin || current > config_.powerCurrentMax)
            throw std::runtime_error("requested current outside configured bounds");
        command("CURR " + toScpiNumber(current));
        return "current set";
    }

  private:
    PowerStatus fakeStatus() const {
        PowerStatus status;
        status.enabled = config_.powerEnabled;
        status.connected = true;
        status.outputEnabled = false;
        status.voltageV = 0.0;
        status.currentA = 0.0;
        status.idn = "FAKE,PDS-USB-SUPPLY,0,0";
        status.fault = "0,No error";
        status.message = "fake power supply status";
        status.lastUpdate = nowIsoLike();
        return status;
    }

    bool ensureOpen(PowerStatus &status) {
        if(fd_ >= 0)
            return true;
        devicePath_ = selectDevice();
        if(devicePath_.empty()) {
            status.message = "no USBTMC or serial power supply device found";
            return false;
        }
        fd_ = open(devicePath_.c_str(), O_RDWR | O_NOCTTY | O_CLOEXEC);
        if(fd_ < 0) {
            status.message = "opening " + devicePath_ + " failed: " + std::strerror(errno);
            return false;
        }
        if(devicePath_.find("/dev/tty") == 0)
            configureSerial();
        return true;
    }

    std::string selectDevice() const {
        if(config_.powerDevice != "auto")
            return config_.powerDevice;
        const auto usbtmc = globPaths("/dev/usbtmc*");
        if(!usbtmc.empty())
            return usbtmc.front();
        const auto serial = globPaths("/dev/ttyUSB*");
        if(!serial.empty())
            return serial.front();
        return "";
    }

    void configureSerial() {
        termios tty{};
        if(tcgetattr(fd_, &tty) != 0)
            throw std::runtime_error("tcgetattr failed for " + devicePath_);
        cfmakeraw(&tty);
        const auto speed = baudToTermios(config_.powerSerialBaud);
        cfsetispeed(&tty, speed);
        cfsetospeed(&tty, speed);
        tty.c_cflag |= CLOCAL | CREAD;
        tty.c_cflag &= ~CRTSCTS;
        tty.c_cc[VMIN] = 0;
        tty.c_cc[VTIME] = 0;
        if(tcsetattr(fd_, TCSANOW, &tty) != 0)
            throw std::runtime_error("tcsetattr failed for " + devicePath_);
    }

    void closeDevice() {
        if(fd_ >= 0) {
            close(fd_);
            fd_ = -1;
        }
    }

    void requireWrites() const {
        if(!config_.powerWritesEnabled)
            throw std::runtime_error("power writes are disabled in config");
        if(!(config_.powerVoltageMax > config_.powerVoltageMin) ||
           !(config_.powerCurrentMax > config_.powerCurrentMin))
            throw std::runtime_error("power write bounds are not configured");
    }

    void command(const std::string &cmd) {
        PowerStatus dummy;
        if(!ensureOpen(dummy))
            throw std::runtime_error(dummy.message);
        const std::string line = cmd + "\n";
        const auto written = write(fd_, line.data(), line.size());
        if(written < 0 || static_cast<size_t>(written) != line.size())
            throw std::runtime_error("SCPI write failed: " + std::string(std::strerror(errno)));
    }

    std::string query(const std::string &cmd) {
        command(cmd);
        std::string response;
        const auto deadline = std::chrono::steady_clock::now() + std::chrono::milliseconds(config_.powerTimeoutMs);
        char buffer[256];
        while(std::chrono::steady_clock::now() < deadline) {
            fd_set rfds;
            FD_ZERO(&rfds);
            FD_SET(fd_, &rfds);
            timeval tv{};
            tv.tv_sec = 0;
            tv.tv_usec = 50'000;
            const int ready = select(fd_ + 1, &rfds, nullptr, nullptr, &tv);
            if(ready < 0)
                throw std::runtime_error("SCPI select failed: " + std::string(std::strerror(errno)));
            if(ready == 0)
                continue;
            const auto n = read(fd_, buffer, sizeof(buffer));
            if(n < 0)
                throw std::runtime_error("SCPI read failed: " + std::string(std::strerror(errno)));
            if(n == 0)
                continue;
            response.append(buffer, buffer + n);
            if(response.find('\n') != std::string::npos || response.find('\r') != std::string::npos)
                break;
        }
        response = trim(response);
        if(response.empty())
            throw std::runtime_error("SCPI query timed out: " + cmd);
        return response;
    }

    static bool parseBoolish(const std::string &value) {
        const auto cleaned = trim(value);
        if(cleaned.empty())
            return false;
        return cleaned == "1" || cleaned == "ON" || cleaned == "on" || cleaned == "On" ||
               cleaned == "true" || cleaned == "TRUE";
    }

    static double parseFirstDouble(const std::string &value) {
        char *end = nullptr;
        const double parsed = std::strtod(value.c_str(), &end);
        if(end == value.c_str())
            throw std::runtime_error("expected numeric SCPI response, got: " + value);
        return parsed;
    }

    static std::string toScpiNumber(double value) {
        std::ostringstream out;
        out << std::fixed << std::setprecision(6) << value;
        return out.str();
    }

    const Config &config_;
    int fd_ = -1;
    std::string devicePath_;
};

struct NodeIds {
    std::map<std::string, std::string> nodes;
};

UA_Variant makeVariant(bool value) {
    UA_Variant variant;
    UA_Variant_init(&variant);
    UA_Boolean uaValue = value;
    UA_Variant_setScalarCopy(&variant, &uaValue, &UA_TYPES[UA_TYPES_BOOLEAN]);
    return variant;
}

UA_Variant makeVariant(int32_t value) {
    UA_Variant variant;
    UA_Variant_init(&variant);
    UA_Int32 uaValue = value;
    UA_Variant_setScalarCopy(&variant, &uaValue, &UA_TYPES[UA_TYPES_INT32]);
    return variant;
}

UA_Variant makeVariant(uint64_t value) {
    UA_Variant variant;
    UA_Variant_init(&variant);
    UA_UInt64 uaValue = value;
    UA_Variant_setScalarCopy(&variant, &uaValue, &UA_TYPES[UA_TYPES_UINT64]);
    return variant;
}

UA_Variant makeVariant(double value) {
    UA_Variant variant;
    UA_Variant_init(&variant);
    UA_Double uaValue = value;
    UA_Variant_setScalarCopy(&variant, &uaValue, &UA_TYPES[UA_TYPES_DOUBLE]);
    return variant;
}

UA_Variant makeVariant(const std::string &value) {
    UA_Variant variant;
    UA_Variant_init(&variant);
    UA_String uaValue = UA_STRING(const_cast<char *>(value.c_str()));
    UA_Variant_setScalarCopy(&variant, &uaValue, &UA_TYPES[UA_TYPES_STRING]);
    return variant;
}

UA_Variant makeLongVariant(int64_t value) {
    UA_Variant variant;
    UA_Variant_init(&variant);
    const UA_Int64 uaValue = value;
    UA_Variant_setScalarCopy(&variant, &uaValue, &UA_TYPES[UA_TYPES_INT64]);
    return variant;
}

UA_Variant makeDateTimeVariant(UA_DateTime value) {
    UA_Variant variant;
    UA_Variant_init(&variant);
    UA_Variant_setScalarCopy(&variant, &value, &UA_TYPES[UA_TYPES_DATETIME]);
    return variant;
}

UA_Variant makeRegistryInitialValue(pds::registry::ValueType type) {
    switch(type) {
    case pds::registry::ValueType::Boolean: return makeVariant(false);
    case pds::registry::ValueType::Integer: return makeVariant(int32_t{0});
    case pds::registry::ValueType::Long: return makeLongVariant(0);
    case pds::registry::ValueType::Double: return makeVariant(0.0);
    case pds::registry::ValueType::String: return makeVariant(std::string("Unavailable"));
    case pds::registry::ValueType::DateTime: return makeDateTimeVariant(0);
    }
    throw std::runtime_error("unknown registry value type");
}

const UA_NodeId &registryDataType(pds::registry::ValueType type) {
    switch(type) {
    case pds::registry::ValueType::Boolean: return UA_TYPES[UA_TYPES_BOOLEAN].typeId;
    case pds::registry::ValueType::Integer: return UA_TYPES[UA_TYPES_INT32].typeId;
    case pds::registry::ValueType::Long: return UA_TYPES[UA_TYPES_INT64].typeId;
    case pds::registry::ValueType::Double: return UA_TYPES[UA_TYPES_DOUBLE].typeId;
    case pds::registry::ValueType::String: return UA_TYPES[UA_TYPES_STRING].typeId;
    case pds::registry::ValueType::DateTime: return UA_TYPES[UA_TYPES_DATETIME].typeId;
    }
    throw std::runtime_error("unknown registry value type");
}

UA_DateTime unixNsToUaDateTime(uint64_t unixNs) {
    constexpr int64_t kUnixEpochInUaTicks = 116444736000000000LL;
    return static_cast<UA_DateTime>(unixNs / 100ULL) + kUnixEpochInUaTicks;
}

UA_NodeId nodeId(UA_UInt16 ns, const std::string &id) {
    return UA_NODEID_STRING(ns, const_cast<char *>(id.c_str()));
}

UA_NodeId nodeId(UA_UInt16 ns, const char *id) {
    return UA_NODEID_STRING(ns, const_cast<char *>(id));
}

UA_QualifiedName qn(UA_UInt16 ns, const std::string &name) {
    return UA_QUALIFIEDNAME(ns, const_cast<char *>(name.c_str()));
}

UA_LocalizedText text(const std::string &value) {
    return UA_LOCALIZEDTEXT(const_cast<char *>("en-US"), const_cast<char *>(value.c_str()));
}

UA_LocalizedText text(const char *value) {
    return UA_LOCALIZEDTEXT(const_cast<char *>("en-US"), const_cast<char *>(value));
}

void addObject(UA_Server *server, UA_UInt16 ns, const std::string &id, const std::string &browseName,
               const std::string &label, const UA_NodeId &parent) {
    UA_ObjectAttributes attr = UA_ObjectAttributes_default;
    attr.displayName = text(label);
    const UA_StatusCode rc = UA_Server_addObjectNode(
        server, nodeId(ns, id), parent, UA_NODEID_NUMERIC(0, UA_NS0ID_ORGANIZES),
        qn(ns, browseName), UA_NODEID_NUMERIC(0, UA_NS0ID_BASEOBJECTTYPE),
        attr, nullptr, nullptr);
    if(rc != UA_STATUSCODE_GOOD)
        throw std::runtime_error("failed to add OPC-UA object " + id);
}

void addVariable(UA_Server *server, UA_UInt16 ns, const std::string &id, const std::string &browseName,
                 const std::string &label, const UA_NodeId &parent, UA_Variant initialValue) {
    UA_VariableAttributes attr = UA_VariableAttributes_default;
    attr.displayName = text(label);
    attr.description = text(label);
    attr.accessLevel = UA_ACCESSLEVELMASK_READ;
    attr.value = initialValue;
    const UA_StatusCode rc = UA_Server_addVariableNode(
        server, nodeId(ns, id), parent, UA_NODEID_NUMERIC(0, UA_NS0ID_ORGANIZES),
        qn(ns, browseName), UA_NODEID_NUMERIC(0, UA_NS0ID_BASEDATAVARIABLETYPE),
        attr, nullptr, nullptr);
    UA_Variant_clear(&initialValue);
    if(rc != UA_STATUSCODE_GOOD)
        throw std::runtime_error("failed to add OPC-UA variable " + id);
}

void writeValue(UA_Server *server, UA_UInt16 ns, const std::string &id, UA_Variant value) {
    const UA_StatusCode rc = UA_Server_writeValue(server, nodeId(ns, id), value);
    UA_Variant_clear(&value);
    if(rc != UA_STATUSCODE_GOOD)
        std::cerr << "warning: failed to write OPC-UA node " << id << "\n";
}

void writeDataValue(UA_Server *server, UA_UInt16 ns, const std::string &id,
                    UA_Variant value, UA_StatusCode status,
                    UA_DateTime sourceTimestamp = 0) {
    UA_DataValue dataValue;
    UA_DataValue_init(&dataValue);
    dataValue.hasValue = true;
    dataValue.value = value;
    dataValue.hasStatus = true;
    dataValue.status = status;
    if(sourceTimestamp != 0) {
        dataValue.hasSourceTimestamp = true;
        dataValue.sourceTimestamp = sourceTimestamp;
    }
    const UA_StatusCode rc =
        UA_Server_writeDataValue(server, nodeId(ns, id), dataValue);
    UA_Variant_clear(&value);
    if(rc != UA_STATUSCODE_GOOD)
        std::cerr << "warning: failed to write OPC-UA data value " << id << "\n";
}

enum class MethodKind { PowerEnableOutput, PowerSetVoltage, PowerSetCurrent, DaphneControl };

struct BridgeState;
struct DaphneRuntime;

struct MethodContext {
    BridgeState *state = nullptr;
    MethodKind kind = MethodKind::DaphneControl;
    RoleMask allowedRoles = 0;
    bool executable = false;
    DaphneRuntime *board = nullptr;
    ControlPolicyEntry policy;
};

struct DaphneRuntime {
    std::string id;
    std::string endpoint;
    std::unique_ptr<DaphneClient> client;
    DaphneStatus status;
    DaphneStatus lastGoodStatus;
    bool hasLastGoodStatus = false;
    uint64_t lastGoodUpdateNs = 0;
    uint64_t pollErrorCount = 0;
    std::shared_ptr<std::mutex> clientMutex = std::make_shared<std::mutex>();
    std::shared_ptr<std::mutex> statusMutex = std::make_shared<std::mutex>();
};

struct BridgeState {
    Config config;
    zmq::context_t daphneContext{1};
    std::vector<DaphneRuntime> daphnes;
    std::unique_ptr<ScpiPowerSupply> power;
    NodeIds ids;
    UA_UInt16 ns = 1;
    UA_UInt16 controlNs = 1;
    PowerStatus powerStatus;
    std::mutex powerClientMutex;
    std::mutex powerStatusMutex;
    std::atomic_bool pollersRunning{false};
    std::vector<std::thread> pollers;
    std::vector<std::unique_ptr<MethodContext>> methodContexts;
    std::set<std::string> registryNodeIds;
    std::map<std::string, pds::registry::ValueType> registryNodeTypes;
    std::set<std::string> canonicalObjectIds;
};

bool roleAllowed(void *sessionContext, RoleMask allowedRoles) {
    if(!sessionContext)
        return false;
    const auto *credential = static_cast<const Credential *>(sessionContext);
    return (allowedRoles & roleMask(credential->role)) != 0;
}

bool backendAdapterImplemented(const ControlPolicyEntry &entry) {
    static const std::map<std::string, std::string> adapters = {
        {"ConfigureRun", "MT2_CONFIGURE_FE_REQ"},
        {"ConfigureClocks", "MT2_CONFIGURE_CLKS_REQ"},
        {"ConfigureFrontend", "MT2_CONFIGURE_FE_REQ"},
        {"SetControlledBias", "MT2_WRITE_VBIAS_CONTROL_REQ"},
        {"SetAfePowerState", "MT2_SET_AFE_POWERSTATE_REQ"},
        {"ResetAfe", "MT2_DO_AFE_RESET_REQ"},
        {"AlignAfes", "MT2_ALIGN_AFE_REQ"},
        {"SoftwareTrigger", "MT2_DO_SOFTWARE_TRIGGER_REQ"},
        {"DumpSpyBuffers", "MT2_DUMP_SPYBUFFER_REQ"},
        {"WriteAfeRegister", "MT2_WRITE_AFE_REG_REQ"},
    };
    const auto found = adapters.find(entry.operation);
    return found != adapters.end() && found->second == entry.backendMapping;
}

std::string instantiatePolicyNodeId(const ControlPolicyEntry &entry,
                                    const std::string &boardId) {
    const auto separator = entry.nodeIdPattern.find(";s=");
    if(separator == std::string::npos)
        throw std::runtime_error("control-policy NodeId is not a string NodeId: " +
                                 entry.nodeIdPattern);
    std::string id = entry.nodeIdPattern.substr(separator + 3);
    const std::string placeholder = "{BoardId}";
    const auto position = id.find(placeholder);
    if(position == std::string::npos)
        throw std::runtime_error("control-policy NodeId lacks {BoardId}: " +
                                 entry.nodeIdPattern);
    id.replace(position, placeholder.size(), boardId);
    return id;
}

MethodContext *makeMethodContext(BridgeState &state, MethodKind kind,
                                 RoleMask allowedRoles, bool executable) {
    auto context = std::make_unique<MethodContext>();
    context->state = &state;
    context->kind = kind;
    context->allowedRoles = allowedRoles;
    context->executable = executable;
    MethodContext *result = context.get();
    state.methodContexts.push_back(std::move(context));
    return result;
}

void defineNode(NodeIds &ids, const std::string &key, const std::string &nodeIdValue) {
    ids.nodes[key] = nodeIdValue;
}

void defineNodes(NodeIds &ids) {
    defineNode(ids, "bridge.heartbeat", "PDS.NP04.Bridge.Heartbeat");
    defineNode(ids, "bridge.version", "PDS.NP04.Bridge.Version");
    defineNode(ids, "bridge.config", "PDS.NP04.Bridge.Config");
    defineNode(ids, "bridge.board_count", "PDS.NP04.Bridge.BoardCount");
    defineNode(ids, "bridge.worker_count", "PDS.NP04.Bridge.WorkerCount");
    defineNode(ids, "power.connected", "PDS.NP04.PowerSupply.USB0.Status.Connected");
    defineNode(ids, "power.output_enabled", "PDS.NP04.PowerSupply.USB0.Status.OutputEnabled");
    defineNode(ids, "power.voltage", "PDS.NP04.PowerSupply.USB0.Status.VoltageV");
    defineNode(ids, "power.current", "PDS.NP04.PowerSupply.USB0.Status.CurrentA");
    defineNode(ids, "power.idn", "PDS.NP04.PowerSupply.USB0.Status.Idn");
    defineNode(ids, "power.fault", "PDS.NP04.PowerSupply.USB0.Status.Fault");
    defineNode(ids, "power.message", "PDS.NP04.PowerSupply.USB0.Status.Message");
    defineNode(ids, "power.last_update", "PDS.NP04.PowerSupply.USB0.Status.LastUpdate");
    defineNode(ids, "summary.daphne_ready", "PDS.NP04.Summary.DaphneReady");
    defineNode(ids, "summary.daphne_ready_count", "PDS.NP04.Summary.DaphneReadyCount");
    defineNode(ids, "summary.daphne_total_count", "PDS.NP04.Summary.DaphneTotalCount");
    defineNode(ids, "summary.power_ready", "PDS.NP04.Summary.PowerReady");
    defineNode(ids, "summary.ready", "PDS.NP04.Summary.Ready");
    defineNode(ids, "summary.message", "PDS.NP04.Summary.Message");
}

const std::string &idOf(const BridgeState &state, const std::string &key) {
    return state.ids.nodes.at(key);
}

std::string daphneObjectId(const std::string &boardId) {
    return "PDS.NP04.DAPHNE." + boardId;
}

std::string daphneStatusObjectId(const std::string &boardId) {
    return daphneObjectId(boardId) + ".Status";
}

std::string daphneStatusNodeId(const std::string &boardId, const std::string &name) {
    return daphneStatusObjectId(boardId) + "." + name;
}

void addMethod(UA_Server *server, BridgeState &state, const std::string &id, const std::string &browseName,
               const std::string &label, MethodContext *context, const UA_NodeId &parent);
void addDaphneMethod(UA_Server *server, BridgeState &state, MethodContext *context,
                     const std::string &id, const UA_NodeId &parent);

std::string leafName(const std::string &id) {
    const auto separator = id.rfind('.');
    return separator == std::string::npos ? id : id.substr(separator + 1);
}

void ensureCanonicalObject(UA_Server *server, BridgeState &state,
                           const std::string &id) {
    if(state.canonicalObjectIds.find(id) != state.canonicalObjectIds.end())
        return;
    const auto separator = id.rfind('.');
    std::string parentId;
    UA_NodeId parent = UA_NODEID_NUMERIC(0, UA_NS0ID_OBJECTSFOLDER);
    if(separator != std::string::npos) {
        parentId = id.substr(0, separator);
        ensureCanonicalObject(server, state, parentId);
        parent = nodeId(state.controlNs, parentId);
    }
    const std::string name = leafName(id);
    addObject(server, state.controlNs, id, name, name, parent);
    UA_NodeClass nodeClass = UA_NODECLASS_UNSPECIFIED;
    const UA_StatusCode readBack =
        UA_Server_readNodeClass(server, nodeId(state.controlNs, id), &nodeClass);
    if(readBack != UA_STATUSCODE_GOOD || nodeClass != UA_NODECLASS_OBJECT)
        throw std::runtime_error("canonical object add did not persist: " + id);
    state.canonicalObjectIds.insert(id);
}

void addCanonicalRegistryVariable(UA_Server *server, BridgeState &state,
                                  const pds::registry::ExpandedNode &node) {
    const auto separator = node.nodeId.rfind('.');
    if(separator == std::string::npos)
        throw std::runtime_error("registry variable NodeId has no parent: " + node.nodeId);
    const std::string parentId = node.nodeId.substr(0, separator);
    ensureCanonicalObject(server, state, parentId);

    UA_VariableAttributes attr = UA_VariableAttributes_default;
    attr.displayName = text(node.entry.name);
    std::string description = node.entry.description + "; status=" +
                              node.entry.implementationStatus + "; owner=" +
                              node.entry.controlOwner;
    attr.description = text(description);
    attr.accessLevel = UA_ACCESSLEVELMASK_READ;
    attr.dataType = registryDataType(node.entry.valueType);
    attr.valueRank = UA_VALUERANK_SCALAR;
    attr.value = makeRegistryInitialValue(node.entry.valueType);
    const UA_StatusCode rc = UA_Server_addVariableNode(
        server, nodeId(state.controlNs, node.nodeId),
        nodeId(state.controlNs, parentId), UA_NODEID_NUMERIC(0, UA_NS0ID_ORGANIZES),
        qn(state.controlNs, leafName(node.nodeId)),
        UA_NODEID_NUMERIC(0, UA_NS0ID_BASEDATAVARIABLETYPE), attr, nullptr, nullptr);
    UA_Variant_clear(&attr.value);
    if(rc != UA_STATUSCODE_GOOD)
        throw std::runtime_error("failed to add canonical registry variable " + node.nodeId);
    state.registryNodeIds.insert(node.nodeId);
    state.registryNodeTypes[node.nodeId] = node.entry.valueType;
    writeDataValue(server, state.controlNs, node.nodeId,
                   makeRegistryInitialValue(node.entry.valueType),
                   UA_STATUSCODE_BADWAITINGFORINITIALDATA);
}

void buildNamespace(UA_Server *server, BridgeState &state) {
    state.ns = UA_Server_addNamespace(server, kNamespaceUri);
    state.controlNs = UA_Server_addNamespace(server, kControlNamespaceUri);
    defineNodes(state.ids);

    const UA_NodeId objects = UA_NODEID_NUMERIC(0, UA_NS0ID_OBJECTSFOLDER);
    addObject(server, state.ns, "PDS", "PDS", "PDS", objects);
    addObject(server, state.ns, "PDS.NP04", "NP04", "NP04", nodeId(state.ns, "PDS"));
    addObject(server, state.ns, "PDS.NP04.Bridge", "Bridge", "Bridge", nodeId(state.ns, "PDS.NP04"));
    addObject(server, state.ns, "PDS.NP04.DAPHNE", "DAPHNE", "DAPHNE", nodeId(state.ns, "PDS.NP04"));
    for(const auto &board : state.daphnes) {
        const std::string objectId = daphneObjectId(board.id);
        addObject(server, state.ns, objectId, board.id, "DAPHNE " + board.id,
                  nodeId(state.ns, "PDS.NP04.DAPHNE"));
        addObject(server, state.ns, daphneStatusObjectId(board.id), "Status", "Status",
                  nodeId(state.ns, objectId));
    }
    addObject(server, state.ns, "PDS.NP04.PowerSupply", "PowerSupply", "Power Supply", nodeId(state.ns, "PDS.NP04"));
    addObject(server, state.ns, "PDS.NP04.PowerSupply.USB0", "USB0", "USB0", nodeId(state.ns, "PDS.NP04.PowerSupply"));
    addObject(server, state.ns, "PDS.NP04.PowerSupply.USB0.Status", "Status", "Status", nodeId(state.ns, "PDS.NP04.PowerSupply.USB0"));
    addObject(server, state.ns, "PDS.NP04.PowerSupply.USB0.Commands", "Commands", "Commands", nodeId(state.ns, "PDS.NP04.PowerSupply.USB0"));
    addObject(server, state.ns, "PDS.NP04.Summary", "Summary", "Summary", nodeId(state.ns, "PDS.NP04"));

    addVariable(server, state.ns, idOf(state, "bridge.heartbeat"), "Heartbeat", "Bridge heartbeat", nodeId(state.ns, "PDS.NP04.Bridge"), makeVariant(int32_t{0}));
    addVariable(server, state.ns, idOf(state, "bridge.version"), "Version", "Bridge version", nodeId(state.ns, "PDS.NP04.Bridge"), makeVariant(std::string("0.4.0")));
    addVariable(server, state.ns, idOf(state, "bridge.config"), "Config", "Config path", nodeId(state.ns, "PDS.NP04.Bridge"), makeVariant(state.config.configPath.empty() ? std::string("<defaults>") : state.config.configPath));
    addVariable(server, state.ns, idOf(state, "bridge.board_count"), "BoardCount", "Configured DAPHNE board count", nodeId(state.ns, "PDS.NP04.Bridge"), makeVariant(static_cast<int32_t>(state.daphnes.size())));
    addVariable(server, state.ns, idOf(state, "bridge.worker_count"), "WorkerCount", "Background DAPHNE poll worker count", nodeId(state.ns, "PDS.NP04.Bridge"), makeVariant(static_cast<int32_t>(std::min<size_t>(state.config.daphneWorkerCount, state.daphnes.size()))));

    for(const auto &board : state.daphnes) {
        const std::string daphneStatusId = daphneStatusObjectId(board.id);
        const UA_NodeId daphneStatus = nodeId(state.ns, daphneStatusId);
        addVariable(server, state.ns, daphneStatusNodeId(board.id, "Connected"), "Connected", "DAPHNE ZMQ connected", daphneStatus, makeVariant(false));
        addVariable(server, state.ns, daphneStatusNodeId(board.id, "Success"), "Success", "DAPHNE status success", daphneStatus, makeVariant(false));
        addVariable(server, state.ns, daphneStatusNodeId(board.id, "Message"), "Message", "DAPHNE status message", daphneStatus, makeVariant(std::string("not polled")));
        addVariable(server, state.ns, daphneStatusNodeId(board.id, "LastUpdate"), "LastUpdate", "DAPHNE last update", daphneStatus, makeVariant(std::string("")));
        addVariable(server, state.ns, daphneStatusNodeId(board.id, "FirmwareLoaded"), "FirmwareLoaded", "DAPHNE firmware loaded", daphneStatus, makeVariant(false));
        addVariable(server, state.ns, daphneStatusNodeId(board.id, "FirmwareBuildId"), "FirmwareBuildId", "DAPHNE firmware build id", daphneStatus, makeVariant(std::string("")));
        addVariable(server, state.ns, daphneStatusNodeId(board.id, "RpuAvailable"), "RpuAvailable", "RPU available", daphneStatus, makeVariant(false));
        addVariable(server, state.ns, daphneStatusNodeId(board.id, "RpuRunning"), "RpuRunning", "RPU running", daphneStatus, makeVariant(false));
        addVariable(server, state.ns, daphneStatusNodeId(board.id, "Mmcm0Locked"), "Mmcm0Locked", "MMCM0 locked", daphneStatus, makeVariant(false));
        addVariable(server, state.ns, daphneStatusNodeId(board.id, "Mmcm1Locked"), "Mmcm1Locked", "MMCM1 locked", daphneStatus, makeVariant(false));
        addVariable(server, state.ns, daphneStatusNodeId(board.id, "TestRegValue"), "TestRegValue", "DAPHNE test register value", daphneStatus, makeVariant(uint64_t{0}));
        addVariable(server, state.ns, daphneStatusNodeId(board.id, "TestRegHex"), "TestRegHex", "DAPHNE test register hex", daphneStatus, makeVariant(std::string("0x0000000000000000")));
        addVariable(server, state.ns, daphneStatusNodeId(board.id, "VBias0"), "VBias0", "DAPHNE VBIAS 0", daphneStatus, makeVariant(0.0));
        addVariable(server, state.ns, daphneStatusNodeId(board.id, "VBias1"), "VBias1", "DAPHNE VBIAS 1", daphneStatus, makeVariant(0.0));
        addVariable(server, state.ns, daphneStatusNodeId(board.id, "VBias2"), "VBias2", "DAPHNE VBIAS 2", daphneStatus, makeVariant(0.0));
        addVariable(server, state.ns, daphneStatusNodeId(board.id, "VBias3"), "VBias3", "DAPHNE VBIAS 3", daphneStatus, makeVariant(0.0));
        addVariable(server, state.ns, daphneStatusNodeId(board.id, "VBias4"), "VBias4", "DAPHNE VBIAS 4", daphneStatus, makeVariant(0.0));
        addVariable(server, state.ns, daphneStatusNodeId(board.id, "PowerMinus5V"), "PowerMinus5V", "DAPHNE -5V rail", daphneStatus, makeVariant(0.0));
        addVariable(server, state.ns, daphneStatusNodeId(board.id, "PowerPlus2p5V"), "PowerPlus2p5V", "DAPHNE PDS rail readback", daphneStatus, makeVariant(0.0));
        addVariable(server, state.ns, daphneStatusNodeId(board.id, "PowerCeV"), "PowerCeV", "DAPHNE CE rail readback", daphneStatus, makeVariant(0.0));
        addVariable(server, state.ns, daphneStatusNodeId(board.id, "TemperatureC"), "TemperatureC", "DAPHNE temperature", daphneStatus, makeVariant(0.0));
        addVariable(server, state.ns, daphneStatusNodeId(board.id, "GeneralInfoJson"), "GeneralInfoJson", "DAPHNE general info JSON", daphneStatus, makeVariant(std::string("{}")));
        addVariable(server, state.ns, daphneStatusNodeId(board.id, "RailCount"), "RailCount", "DAPHNE rail count", daphneStatus, makeVariant(int32_t{0}));
        addVariable(server, state.ns, daphneStatusNodeId(board.id, "TemperatureCount"), "TemperatureCount", "DAPHNE temperature count", daphneStatus, makeVariant(int32_t{0}));
        addVariable(server, state.ns, daphneStatusNodeId(board.id, "ErrorCount"), "ErrorCount", "DAPHNE error count", daphneStatus, makeVariant(int32_t{0}));
        addVariable(server, state.ns, daphneStatusNodeId(board.id, "RailsJson"), "RailsJson", "DAPHNE rails JSON", daphneStatus, makeVariant(std::string("[]")));
        addVariable(server, state.ns, daphneStatusNodeId(board.id, "ErrorsJson"), "ErrorsJson", "DAPHNE errors JSON", daphneStatus, makeVariant(std::string("[]")));
    }

    const UA_NodeId powerStatus = nodeId(state.ns, "PDS.NP04.PowerSupply.USB0.Status");
    addVariable(server, state.ns, idOf(state, "power.connected"), "Connected", "Power supply connected", powerStatus, makeVariant(false));
    addVariable(server, state.ns, idOf(state, "power.output_enabled"), "OutputEnabled", "Power supply output enabled", powerStatus, makeVariant(false));
    addVariable(server, state.ns, idOf(state, "power.voltage"), "VoltageV", "Power supply voltage", powerStatus, makeVariant(0.0));
    addVariable(server, state.ns, idOf(state, "power.current"), "CurrentA", "Power supply current", powerStatus, makeVariant(0.0));
    addVariable(server, state.ns, idOf(state, "power.idn"), "Idn", "Power supply identity", powerStatus, makeVariant(std::string("")));
    addVariable(server, state.ns, idOf(state, "power.fault"), "Fault", "Power supply fault", powerStatus, makeVariant(std::string("")));
    addVariable(server, state.ns, idOf(state, "power.message"), "Message", "Power supply message", powerStatus, makeVariant(std::string("not polled")));
    addVariable(server, state.ns, idOf(state, "power.last_update"), "LastUpdate", "Power supply last update", powerStatus, makeVariant(std::string("")));

    const UA_NodeId summary = nodeId(state.ns, "PDS.NP04.Summary");
    addVariable(server, state.ns, idOf(state, "summary.daphne_ready"), "DaphneReady", "DAPHNE ready", summary, makeVariant(false));
    addVariable(server, state.ns, idOf(state, "summary.daphne_ready_count"), "DaphneReadyCount", "Ready DAPHNE board count", summary, makeVariant(int32_t{0}));
    addVariable(server, state.ns, idOf(state, "summary.daphne_total_count"), "DaphneTotalCount", "Configured DAPHNE board count", summary, makeVariant(static_cast<int32_t>(state.daphnes.size())));
    addVariable(server, state.ns, idOf(state, "summary.power_ready"), "PowerReady", "Power ready", summary, makeVariant(false));
    addVariable(server, state.ns, idOf(state, "summary.ready"), "Ready", "Combined ready", summary, makeVariant(false));
    addVariable(server, state.ns, idOf(state, "summary.message"), "Message", "Combined status message", summary, makeVariant(std::string("not polled")));

    const RoleMask scRoles = roleMask(ClientRole::SlowControls) | roleMask(ClientRole::Expert);
    addMethod(server, state, "PDS.NP04.PowerSupply.USB0.Commands.EnableOutput", "EnableOutput", "Enable output",
              makeMethodContext(state, MethodKind::PowerEnableOutput, scRoles,
                                state.config.powerWritesEnabled),
              nodeId(state.ns, "PDS.NP04.PowerSupply.USB0.Commands"));
    addMethod(server, state, "PDS.NP04.PowerSupply.USB0.Commands.SetVoltage", "SetVoltage", "Set voltage",
              makeMethodContext(state, MethodKind::PowerSetVoltage, scRoles,
                                state.config.powerWritesEnabled),
              nodeId(state.ns, "PDS.NP04.PowerSupply.USB0.Commands"));
    addMethod(server, state, "PDS.NP04.PowerSupply.USB0.Commands.SetCurrent", "SetCurrent", "Set current",
              makeMethodContext(state, MethodKind::PowerSetCurrent, scRoles,
                                state.config.powerWritesEnabled),
              nodeId(state.ns, "PDS.NP04.PowerSupply.USB0.Commands"));

    /* Canonical workbook-driven namespace. Keep the NP04 namespace above as
     * a compatibility projection while clients migrate to the v8 contract. */
    ensureCanonicalObject(server, state, "DAPHNE");
    ensureCanonicalObject(server, state, "DAPHNE.Gateway");
    ensureCanonicalObject(server, state, "DAPHNE.Boards");
    addVariable(server, state.controlNs, "DAPHNE.Gateway.PolicyPath", "PolicyPath",
                "Workbook-exported control policy", nodeId(state.controlNs, "DAPHNE.Gateway"),
                makeVariant(state.config.controlPolicyPath));
    addVariable(server, state.controlNs, "DAPHNE.Gateway.PolicyOperationCount",
                "PolicyOperationCount", "Number of policy-controlled operations",
                nodeId(state.controlNs, "DAPHNE.Gateway"),
                makeVariant(static_cast<int32_t>(state.config.controlPolicy.size())));
    addVariable(server, state.controlNs, "DAPHNE.Gateway.WritesEnabled", "WritesEnabled",
                "DAPHNE writes enabled", nodeId(state.controlNs, "DAPHNE.Gateway"),
                makeVariant(state.config.daphneWritesEnabled));
    addVariable(server, state.controlNs, "DAPHNE.Gateway.RegistryPath", "RegistryPath",
                "Workbook-exported variable registry", nodeId(state.controlNs, "DAPHNE.Gateway"),
                makeVariant(state.config.registryTagListPath));
    addVariable(server, state.controlNs, "DAPHNE.Gateway.RegistryPatternCount",
                "RegistryPatternCount", "Number of registry variable/method patterns",
                nodeId(state.controlNs, "DAPHNE.Gateway"),
                makeVariant(static_cast<int32_t>(state.config.registryEntries.size())));
    addVariable(server, state.controlNs, "DAPHNE.Gateway.RegistryNodeCount",
                "RegistryNodeCount", "Number of expanded registry nodes",
                nodeId(state.controlNs, "DAPHNE.Gateway"),
                makeVariant(static_cast<int32_t>(state.config.registryNodes.size())));

    for(auto &board : state.daphnes) {
        const std::string boardObject = "DAPHNE.Boards." + board.id;
        const std::string operationsObject = boardObject + ".Operations";
        const std::string methodsObject = operationsObject + ".Methods";
        ensureCanonicalObject(server, state, boardObject);
        ensureCanonicalObject(server, state, operationsObject);
        ensureCanonicalObject(server, state, methodsObject);
    }

    for(const auto &node : state.config.registryNodes) {
        if(node.entry.access == pds::registry::Access::ReadOnly)
            addCanonicalRegistryVariable(server, state, node);
    }

    for(auto &board : state.daphnes) {
        const std::string methodsObject =
            "DAPHNE.Boards." + board.id + ".Operations.Methods";
        for(const auto &policy : state.config.controlPolicy) {
            MethodContext *context = makeMethodContext(
                state, MethodKind::DaphneControl, policy.allowedRoles,
                state.config.daphneWritesEnabled && policy.executable &&
                    backendAdapterImplemented(policy));
            context->board = &board;
            context->policy = policy;
            addDaphneMethod(server, state, context,
                             instantiatePolicyNodeId(policy, board.id),
                             nodeId(state.controlNs, methodsObject));
        }
    }
}

void writeCanonical(UA_Server *server, BridgeState &state, const std::string &id,
                    UA_Variant value, UA_StatusCode status = UA_STATUSCODE_GOOD,
                    UA_DateTime sourceTimestamp = 0) {
    if(state.registryNodeIds.find(id) == state.registryNodeIds.end()) {
        UA_Variant_clear(&value);
        return;
    }
    writeDataValue(server, state.controlNs, id, value, status, sourceTimestamp);
}
void updateNodes(UA_Server *server, BridgeState &state) {
    static int32_t heartbeat = 0;
    ++heartbeat;
    writeValue(server, state.ns, idOf(state, "bridge.heartbeat"), makeVariant(heartbeat));

    const pds::bridge::OpcUaSnapshotWriter snapshot_writer(
        server, state.controlNs, state.registryNodeTypes);
    int32_t daphneReadyCount = 0;
    for(auto &board : state.daphnes) {
        DaphneStatus d;
        DaphneStatus lastGood;
        bool hasLastGood = false;
        uint64_t lastGoodUpdateNs = 0;
        uint64_t pollErrorCount = 0;
        {
            std::lock_guard<std::mutex> lock(*board.statusMutex);
            d = board.status;
            lastGood = board.lastGoodStatus;
            hasLastGood = board.hasLastGoodStatus;
            lastGoodUpdateNs = board.lastGoodUpdateNs;
            pollErrorCount = board.pollErrorCount;
        }
        if(!state.config.daphneEnabled || (d.connected && d.success))
            ++daphneReadyCount;
        writeValue(server, state.ns, daphneStatusNodeId(board.id, "Connected"), makeVariant(d.connected));
        writeValue(server, state.ns, daphneStatusNodeId(board.id, "Success"), makeVariant(d.success));
        writeValue(server, state.ns, daphneStatusNodeId(board.id, "Message"), makeVariant(d.message));
        writeValue(server, state.ns, daphneStatusNodeId(board.id, "LastUpdate"), makeVariant(d.lastUpdate));
        writeValue(server, state.ns, daphneStatusNodeId(board.id, "FirmwareLoaded"), makeVariant(d.firmwareLoaded));
        writeValue(server, state.ns, daphneStatusNodeId(board.id, "FirmwareBuildId"), makeVariant(d.firmwareBuildId));
        writeValue(server, state.ns, daphneStatusNodeId(board.id, "RpuAvailable"), makeVariant(d.rpuAvailable));
        writeValue(server, state.ns, daphneStatusNodeId(board.id, "RpuRunning"), makeVariant(d.rpuRunning));
        writeValue(server, state.ns, daphneStatusNodeId(board.id, "Mmcm0Locked"), makeVariant(d.mmcm0Locked));
        writeValue(server, state.ns, daphneStatusNodeId(board.id, "Mmcm1Locked"), makeVariant(d.mmcm1Locked));
        writeValue(server, state.ns, daphneStatusNodeId(board.id, "TestRegValue"), makeVariant(d.testRegValue));
        writeValue(server, state.ns, daphneStatusNodeId(board.id, "TestRegHex"), makeVariant(d.testRegHex));
        writeValue(server, state.ns, daphneStatusNodeId(board.id, "VBias0"), makeVariant(d.vBias0));
        writeValue(server, state.ns, daphneStatusNodeId(board.id, "VBias1"), makeVariant(d.vBias1));
        writeValue(server, state.ns, daphneStatusNodeId(board.id, "VBias2"), makeVariant(d.vBias2));
        writeValue(server, state.ns, daphneStatusNodeId(board.id, "VBias3"), makeVariant(d.vBias3));
        writeValue(server, state.ns, daphneStatusNodeId(board.id, "VBias4"), makeVariant(d.vBias4));
        writeValue(server, state.ns, daphneStatusNodeId(board.id, "PowerMinus5V"), makeVariant(d.powerMinus5V));
        writeValue(server, state.ns, daphneStatusNodeId(board.id, "PowerPlus2p5V"), makeVariant(d.powerPlus2p5V));
        writeValue(server, state.ns, daphneStatusNodeId(board.id, "PowerCeV"), makeVariant(d.powerCeV));
        writeValue(server, state.ns, daphneStatusNodeId(board.id, "TemperatureC"), makeVariant(d.temperatureC));
        writeValue(server, state.ns, daphneStatusNodeId(board.id, "GeneralInfoJson"), makeVariant(d.generalInfo));
        writeValue(server, state.ns, daphneStatusNodeId(board.id, "RailCount"), makeVariant(d.railCount));
        writeValue(server, state.ns, daphneStatusNodeId(board.id, "TemperatureCount"), makeVariant(d.temperatureCount));
        writeValue(server, state.ns, daphneStatusNodeId(board.id, "ErrorCount"), makeVariant(d.errorCount));
        writeValue(server, state.ns, daphneStatusNodeId(board.id, "RailsJson"), makeVariant(d.rails));
        writeValue(server, state.ns, daphneStatusNodeId(board.id, "ErrorsJson"), makeVariant(d.errors));

        const uint64_t currentNs = nowNs();
        const double ageSeconds = hasLastGood && currentNs >= lastGoodUpdateNs
                                      ? static_cast<double>(currentNs - lastGoodUpdateNs) / 1.0e9
                                      : 0.0;
        const bool currentGood = d.connected && d.success;
        const bool stale = !currentGood && hasLastGood &&
                           ageSeconds * 1000.0 >= state.config.registryStaleAfterMs;
        const std::string quality = currentGood ? "Good"
                                    : stale ? "Stale"
                                    : hasLastGood ? "Uncertain"
                                                  : "Bad";
        const UA_StatusCode measurementStatus =
            currentGood ? UA_STATUSCODE_GOOD
                        : hasLastGood ? UA_STATUSCODE_UNCERTAINLASTUSABLEVALUE
                                      : UA_STATUSCODE_BADNOTCONNECTED;
        const DaphneStatus &measurement = hasLastGood ? lastGood : d;
        const UA_DateTime currentTimestamp =
            d.updateTimeNs == 0 ? UA_DateTime_now() : unixNsToUaDateTime(d.updateTimeNs);
        const UA_DateTime measurementTimestamp =
            lastGoodUpdateNs == 0 ? 0 : unixNsToUaDateTime(lastGoodUpdateNs);
        const std::string base = "DAPHNE.Boards." + board.id + ".";

        writeCanonical(server, state, base + "Status.Connected", makeVariant(d.connected),
                       UA_STATUSCODE_GOOD, currentTimestamp);
        writeCanonical(server, state, base + "Status.Success", makeVariant(d.success),
                       UA_STATUSCODE_GOOD, currentTimestamp);
        writeCanonical(server, state, base + "Status.Message", makeVariant(d.message),
                       UA_STATUSCODE_GOOD, currentTimestamp);
        writeCanonical(server, state, base + "Status.LastUpdate",
                       makeDateTimeVariant(currentTimestamp), UA_STATUSCODE_GOOD,
                       currentTimestamp);
        writeCanonical(server, state, base + "Status.Quality", makeVariant(quality),
                       UA_STATUSCODE_GOOD, currentTimestamp);
        if(hasLastGood) {
            writeCanonical(server, state, base + "Status.AgeSeconds",
                           makeVariant(ageSeconds), UA_STATUSCODE_GOOD,
                           measurementTimestamp);
        }
        writeCanonical(server, state, base + "Status.ErrorCount",
                       makeVariant(static_cast<int32_t>(d.errorCount +
                                                       (currentGood ? 0 : 1))),
                       UA_STATUSCODE_GOOD, currentTimestamp);
        writeCanonical(server, state, base + "Status.ErrorsJson", makeVariant(d.errors),
                       UA_STATUSCODE_GOOD, currentTimestamp);

        writeCanonical(server, state, base + "Bridge.Heartbeat",
                       makeLongVariant(heartbeat), UA_STATUSCODE_GOOD,
                       currentTimestamp);
        writeCanonical(server, state, base + "Bridge.Version",
                       makeVariant(std::string("0.4.0")), UA_STATUSCODE_GOOD,
                       currentTimestamp);
        writeCanonical(server, state, base + "Bridge.ConfigurationPath",
                       makeVariant(state.config.configPath), UA_STATUSCODE_GOOD,
                       currentTimestamp);
        writeCanonical(server, state, base + "Bridge.NamespaceVersion",
                       makeVariant(state.config.registryNamespaceVersion),
                       UA_STATUSCODE_GOOD, currentTimestamp);
        writeCanonical(server, state, base + "Bridge.BoardCount",
                       makeVariant(static_cast<int32_t>(state.daphnes.size())),
                       UA_STATUSCODE_GOOD, currentTimestamp);
        writeCanonical(server, state, base + "Bridge.WorkerCount",
                       makeVariant(static_cast<int32_t>(std::min<size_t>(
                           state.config.daphneWorkerCount, state.daphnes.size()))),
                       UA_STATUSCODE_GOOD, currentTimestamp);
        writeCanonical(server, state, base + "Bridge.PollErrorCount",
                       makeLongVariant(static_cast<int64_t>(pollErrorCount)),
                       UA_STATUSCODE_GOOD, currentTimestamp);
        writeCanonical(server, state, base + "Bridge.BackendProtocolVersion",
                       makeVariant(measurement.telemetryV8
                                       ? std::string("ControlEnvelopeV2+daphne.telemetry.v8/2.0")
                                       : std::string("ControlEnvelopeV2")),
                       UA_STATUSCODE_GOOD, currentTimestamp);
        writeCanonical(server, state, base + "Bridge.BackendCompatibilityState",
                       makeVariant(measurement.telemetryV8
                                       ? std::string("ProposedV8Telemetry")
                                       : std::string("LegacyReadbackSubset")),
                       UA_STATUSCODE_GOOD, currentTimestamp);

        if(measurement.telemetryV8) {
            const size_t translation_errors =
                snapshot_writer.Publish(measurement.telemetry, currentGood);
            if(translation_errors != 0) {
                std::cerr << "warning: rejected " << translation_errors
                          << " malformed or unknown v8 telemetry points for DAPHNE "
                          << board.id << "\n";
            }
        } else {
            // Older boards expose only this small scalar subset. Native v8
            // snapshots bypass this compatibility adapter entirely.
            const double biasValues[] = {
                measurement.vBias0, measurement.vBias1, measurement.vBias2,
                measurement.vBias3, measurement.vBias4,
            };
            for(size_t afe = 0; afe < 5; ++afe) {
                writeCanonical(server, state,
                               base + "AFE.Blocks." + std::to_string(afe) + ".BiasVoltage",
                               makeVariant(biasValues[afe]), measurementStatus,
                               measurementTimestamp);
            }
            writeCanonical(server, state, base + "Power.BoardRails.Minus5VA.Voltage",
                           makeVariant(measurement.powerMinus5V), measurementStatus,
                           measurementTimestamp);
            writeCanonical(server, state, base + "Power.BoardRails.3V3PDS.Voltage",
                           makeVariant(measurement.powerPlus2p5V), measurementStatus,
                           measurementTimestamp);
            writeCanonical(server, state, base + "Power.BoardRails.1V8A.Voltage",
                           makeVariant(measurement.powerCeV), measurementStatus,
                           measurementTimestamp);
            for(const auto &rail : {std::string("Minus5VA"), std::string("3V3PDS"),
                                    std::string("1V8A")}) {
                writeCanonical(server, state, base + "Power.BoardRails." + rail + ".Status",
                               makeVariant(quality), measurementStatus,
                               measurementTimestamp);
            }
        }
    }

    PowerStatus p;
    {
        std::lock_guard<std::mutex> lock(state.powerStatusMutex);
        p = state.powerStatus;
    }
    writeValue(server, state.ns, idOf(state, "power.connected"), makeVariant(p.connected));
    writeValue(server, state.ns, idOf(state, "power.output_enabled"), makeVariant(p.outputEnabled));
    writeValue(server, state.ns, idOf(state, "power.voltage"), makeVariant(p.voltageV));
    writeValue(server, state.ns, idOf(state, "power.current"), makeVariant(p.currentA));
    writeValue(server, state.ns, idOf(state, "power.idn"), makeVariant(p.idn));
    writeValue(server, state.ns, idOf(state, "power.fault"), makeVariant(p.fault));
    writeValue(server, state.ns, idOf(state, "power.message"), makeVariant(p.message));
    writeValue(server, state.ns, idOf(state, "power.last_update"), makeVariant(p.lastUpdate));

    const bool daphneReady = daphneReadyCount == static_cast<int32_t>(state.daphnes.size());
    const bool powerReady = !state.config.powerEnabled || p.connected;
    const bool ready = daphneReady && powerReady;
    std::ostringstream summary;
    summary << "daphne=" << daphneReadyCount << "/" << state.daphnes.size()
            << " ready; power=" << p.message;
    writeValue(server, state.ns, idOf(state, "summary.daphne_ready"), makeVariant(daphneReady));
    writeValue(server, state.ns, idOf(state, "summary.daphne_ready_count"), makeVariant(daphneReadyCount));
    writeValue(server, state.ns, idOf(state, "summary.daphne_total_count"), makeVariant(static_cast<int32_t>(state.daphnes.size())));
    writeValue(server, state.ns, idOf(state, "summary.power_ready"), makeVariant(powerReady));
    writeValue(server, state.ns, idOf(state, "summary.ready"), makeVariant(ready));
    writeValue(server, state.ns, idOf(state, "summary.message"), makeVariant(summary.str()));
}

void daphnePollWorker(BridgeState *state, size_t workerIndex, size_t workerCount) {
    while(state->pollersRunning.load()) {
        const auto cycleStart = std::chrono::steady_clock::now();
        for(size_t index = workerIndex;
            index < state->daphnes.size() && state->pollersRunning.load();
            index += workerCount) {
            auto &board = state->daphnes[index];
            DaphneStatus status;
            {
                std::lock_guard<std::mutex> clientLock(*board.clientMutex);
                status = board.client->Poll();
            }
            std::lock_guard<std::mutex> lock(*board.statusMutex);
            if(status.connected && status.success) {
                board.lastGoodStatus = status;
                board.hasLastGoodStatus = true;
                board.lastGoodUpdateNs = status.updateTimeNs;
            } else {
                ++board.pollErrorCount;
            }
            board.status = std::move(status);
        }
        const auto nextCycle =
            cycleStart + std::chrono::milliseconds(state->config.pollPeriodMs);
        while(state->pollersRunning.load() &&
              std::chrono::steady_clock::now() < nextCycle) {
            std::this_thread::sleep_for(std::chrono::milliseconds(20));
        }
    }
}

void powerPollWorker(BridgeState *state) {
    while(state->pollersRunning.load()) {
        const auto cycleStart = std::chrono::steady_clock::now();
        PowerStatus status;
        {
            std::lock_guard<std::mutex> lock(state->powerClientMutex);
            status = state->power->poll();
        }
        {
            std::lock_guard<std::mutex> lock(state->powerStatusMutex);
            state->powerStatus = std::move(status);
        }
        const auto nextCycle =
            cycleStart + std::chrono::milliseconds(state->config.pollPeriodMs);
        while(state->pollersRunning.load() &&
              std::chrono::steady_clock::now() < nextCycle) {
            std::this_thread::sleep_for(std::chrono::milliseconds(20));
        }
    }
}

void startPollers(BridgeState &state) {
    state.pollersRunning.store(true);
    const size_t workerCount =
        std::min<size_t>(state.config.daphneWorkerCount, state.daphnes.size());
    state.pollers.reserve(workerCount + 1);
    for(size_t index = 0; index < workerCount; ++index)
        state.pollers.emplace_back(daphnePollWorker, &state, index, workerCount);
    state.pollers.emplace_back(powerPollWorker, &state);
}

void stopPollers(BridgeState &state) {
    state.pollersRunning.store(false);
    for(auto &poller : state.pollers) {
        if(poller.joinable())
            poller.join();
    }
    state.pollers.clear();
}

void pollCallback(UA_Server *server, void *data) {
    auto *state = static_cast<BridgeState *>(data);
    try {
        updateNodes(server, *state);
    } catch(const std::exception &err) {
        std::cerr << "poll failed: " << err.what() << "\n";
    }
}

UA_StatusCode writeMethodOutput(UA_Variant *output, const std::string &message) {
    UA_String result = UA_STRING(const_cast<char *>(message.c_str()));
    return UA_Variant_setScalarCopy(output, &result, &UA_TYPES[UA_TYPES_STRING]);
}

UA_StatusCode powerMethodCallback(UA_Server *server, const UA_NodeId *, void *sessionContext,
                                  const UA_NodeId *, void *methodContext,
                                  const UA_NodeId *, void *, size_t inputSize,
                                  const UA_Variant *input, size_t outputSize,
                                  UA_Variant *output) {
    auto *context = static_cast<MethodContext *>(methodContext);
    if(!context || !context->state || outputSize < 1)
        return UA_STATUSCODE_BADINTERNALERROR;
    if(!context->executable)
        return UA_STATUSCODE_BADNOTSUPPORTED;
    if(!roleAllowed(sessionContext, context->allowedRoles))
        return UA_STATUSCODE_BADUSERACCESSDENIED;
    try {
        std::string message;
        switch(context->kind) {
        case MethodKind::PowerEnableOutput: {
            if(inputSize != 1 || !UA_Variant_hasScalarType(&input[0], &UA_TYPES[UA_TYPES_BOOLEAN]))
                return UA_STATUSCODE_BADINVALIDARGUMENT;
            const bool enabled = *static_cast<UA_Boolean *>(input[0].data);
            std::lock_guard<std::mutex> lock(context->state->powerClientMutex);
            message = context->state->power->setOutput(enabled);
            break;
        }
        case MethodKind::PowerSetVoltage: {
            if(inputSize != 1 || !UA_Variant_hasScalarType(&input[0], &UA_TYPES[UA_TYPES_DOUBLE]))
                return UA_STATUSCODE_BADINVALIDARGUMENT;
            const double voltage = *static_cast<UA_Double *>(input[0].data);
            std::lock_guard<std::mutex> lock(context->state->powerClientMutex);
            message = context->state->power->setVoltage(voltage);
            break;
        }
        case MethodKind::PowerSetCurrent: {
            if(inputSize != 1 || !UA_Variant_hasScalarType(&input[0], &UA_TYPES[UA_TYPES_DOUBLE]))
                return UA_STATUSCODE_BADINVALIDARGUMENT;
            const double current = *static_cast<UA_Double *>(input[0].data);
            std::lock_guard<std::mutex> lock(context->state->powerClientMutex);
            message = context->state->power->setCurrent(current);
            break;
        }
        case MethodKind::DaphneControl:
            return UA_STATUSCODE_BADINTERNALERROR;
        }
        updateNodes(server, *context->state);
        return writeMethodOutput(output, message);
    } catch(const std::exception &err) {
        return writeMethodOutput(output, std::string("rejected: ") + err.what());
    }
}

std::string uaString(const UA_String &value) {
    return std::string(reinterpret_cast<const char *>(value.data), value.length);
}

UA_StatusCode daphneMethodCallback(UA_Server *, const UA_NodeId *, void *sessionContext,
                                   const UA_NodeId *, void *methodContext,
                                   const UA_NodeId *, void *, size_t inputSize,
                                   const UA_Variant *input, size_t outputSize,
                                   UA_Variant *output) {
    auto *context = static_cast<MethodContext *>(methodContext);
    if(!context || !context->state || !context->board || outputSize < 1)
        return UA_STATUSCODE_BADINTERNALERROR;
    if(!context->executable)
        return UA_STATUSCODE_BADNOTSUPPORTED;
    if(!roleAllowed(sessionContext, context->allowedRoles))
        return UA_STATUSCODE_BADUSERACCESSDENIED;
    if(inputSize != 1 ||
       !UA_Variant_hasScalarType(&input[0], &UA_TYPES[UA_TYPES_STRING]))
        return UA_STATUSCODE_BADINVALIDARGUMENT;

    const auto &request = *static_cast<const UA_String *>(input[0].data);
    try {
        std::lock_guard<std::mutex> lock(*context->board->clientMutex);
        const std::string response = context->board->client->Execute(
            context->policy.operation, uaString(request));
        return writeMethodOutput(output, response);
    } catch(const std::exception &err) {
        std::ostringstream response;
        response << "{\"success\":false,\"message\":\"";
        for(const char ch : std::string(err.what())) {
            if(ch == '"' || ch == '\\')
                response << '\\';
            response << ch;
        }
        response << "\"}";
        return writeMethodOutput(output, response.str());
    }
}

void addMethod(UA_Server *server, BridgeState &state, const std::string &id, const std::string &browseName,
               const std::string &label, MethodContext *context, const UA_NodeId &parent) {
#ifdef UA_ENABLE_METHODCALLS
    UA_MethodAttributes attr = UA_MethodAttributes_default;
    attr.displayName = text(label);
    attr.executable = context->executable;
    attr.userExecutable = context->executable;

    UA_Argument inputArg;
    UA_Argument_init(&inputArg);
    inputArg.name = UA_STRING(const_cast<char *>("value"));
    inputArg.description = text("command value");
    inputArg.valueRank = UA_VALUERANK_SCALAR;
    inputArg.dataType = context->kind == MethodKind::PowerEnableOutput
                            ? UA_TYPES[UA_TYPES_BOOLEAN].typeId
                            : UA_TYPES[UA_TYPES_DOUBLE].typeId;

    UA_Argument outputArg;
    UA_Argument_init(&outputArg);
    outputArg.name = UA_STRING(const_cast<char *>("message"));
    outputArg.description = text("command result");
    outputArg.valueRank = UA_VALUERANK_SCALAR;
    outputArg.dataType = UA_TYPES[UA_TYPES_STRING].typeId;

    const UA_StatusCode rc = UA_Server_addMethodNode(
        server, nodeId(state.ns, id), parent, UA_NODEID_NUMERIC(0, UA_NS0ID_HASCOMPONENT),
        qn(state.ns, browseName), attr, powerMethodCallback, 1, &inputArg, 1, &outputArg, context, nullptr);
    if(rc != UA_STATUSCODE_GOOD)
        throw std::runtime_error("failed to add OPC-UA method " + id);
#else
    (void)server;
    (void)state;
    (void)id;
    (void)browseName;
    (void)label;
    (void)context;
    (void)parent;
#endif
}

void addDaphneMethod(UA_Server *server, BridgeState &state, MethodContext *context,
                     const std::string &id, const UA_NodeId &parent) {
#ifdef UA_ENABLE_METHODCALLS
    UA_MethodAttributes attr = UA_MethodAttributes_default;
    attr.displayName = text(context->policy.operation);
    const std::string description =
        context->policy.owner + "; backend=" +
        (context->policy.backendMapping.empty() ? std::string("not implemented")
                                                : context->policy.backendMapping);
    attr.description = text(description);
    attr.executable = context->executable;
    attr.userExecutable = context->executable;

    UA_Argument inputArg;
    UA_Argument_init(&inputArg);
    inputArg.name = UA_STRING(const_cast<char *>("requestJson"));
    inputArg.description = text("JSON object matching the mapped DAPHNE request");
    inputArg.valueRank = UA_VALUERANK_SCALAR;
    inputArg.dataType = UA_TYPES[UA_TYPES_STRING].typeId;

    UA_Argument outputArg;
    UA_Argument_init(&outputArg);
    outputArg.name = UA_STRING(const_cast<char *>("responseJson"));
    outputArg.description = text("JSON object containing success, message and readback fields");
    outputArg.valueRank = UA_VALUERANK_SCALAR;
    outputArg.dataType = UA_TYPES[UA_TYPES_STRING].typeId;

    const UA_StatusCode rc = UA_Server_addMethodNode(
        server, nodeId(state.controlNs, id), parent,
        UA_NODEID_NUMERIC(0, UA_NS0ID_HASCOMPONENT),
        qn(state.controlNs, context->policy.operation), attr, daphneMethodCallback,
        1, &inputArg, 1, &outputArg, context, nullptr);
    if(rc != UA_STATUSCODE_GOOD)
        throw std::runtime_error("failed to add workbook OPC-UA method " + id);
#else
    (void)server;
    (void)state;
    (void)context;
    (void)id;
    (void)parent;
#endif
}

bool uaStringEquals(const UA_String *value, const std::string &expected) {
    return value && value->length == expected.size() &&
           std::memcmp(value->data, expected.data(), expected.size()) == 0;
}

UA_StatusCode loginCallback(const UA_String *username, const UA_ByteString *password,
                            size_t, const UA_UsernamePasswordLogin *,
                            void **sessionContext, void *loginContext) {
    auto *state = static_cast<BridgeState *>(loginContext);
    if(!state || !sessionContext)
        return UA_STATUSCODE_BADINTERNALERROR;
    for(auto &credential : state->config.credentials) {
        if(uaStringEquals(username, credential.username) &&
           uaStringEquals(password, credential.password)) {
            *sessionContext = &credential;
            return UA_STATUSCODE_GOOD;
        }
    }
    return UA_STATUSCODE_BADUSERACCESSDENIED;
}

UA_Boolean getUserExecutable(UA_Server *, UA_AccessControl *, const UA_NodeId *,
                             void *sessionContext, const UA_NodeId *,
                             void *methodContext) {
    const auto *context = static_cast<const MethodContext *>(methodContext);
    return context && context->executable &&
           roleAllowed(sessionContext, context->allowedRoles);
}

UA_Boolean getUserExecutableOnObject(UA_Server *, UA_AccessControl *,
                                     const UA_NodeId *, void *sessionContext,
                                     const UA_NodeId *, void *methodContext,
                                     const UA_NodeId *, void *) {
    const auto *context = static_cast<const MethodContext *>(methodContext);
    return context && context->executable &&
           roleAllowed(sessionContext, context->allowedRoles);
}

void configureAccessControl(UA_ServerConfig *serverConfig, BridgeState &state) {
    std::vector<UA_UsernamePasswordLogin> logins(state.config.credentials.size());
    for(size_t index = 0; index < state.config.credentials.size(); ++index) {
        logins[index].username = UA_STRING(
            const_cast<char *>(state.config.credentials[index].username.c_str()));
        logins[index].password = UA_STRING(
            const_cast<char *>(state.config.credentials[index].password.c_str()));
    }
    const UA_StatusCode rc = UA_AccessControl_defaultWithLoginCallback(
        serverConfig, state.config.opcuaAllowAnonymousRead, nullptr, logins.size(),
        logins.empty() ? nullptr : logins.data(), loginCallback, &state);
    if(rc != UA_STATUSCODE_GOOD)
        throw std::runtime_error("failed to configure OPC-UA authentication");
    serverConfig->accessControl.getUserExecutable = getUserExecutable;
    serverConfig->accessControl.getUserExecutableOnObject = getUserExecutableOnObject;
}

void usage() {
    std::cout << "usage: pds-opcua-bridge [--config PATH] [--fake] [--help]\n";
}

} // namespace

int main(int argc, char **argv) {
    std::string configPath;
    bool forceFake = false;
    for(int i = 1; i < argc; ++i) {
        const std::string arg = argv[i];
        if(arg == "--config") {
            if(i + 1 >= argc) {
                std::cerr << "--config requires a path\n";
                return 2;
            }
            configPath = argv[++i];
        } else if(arg == "--fake") {
            forceFake = true;
        } else if(arg == "--help" || arg == "-h") {
            usage();
            return 0;
        } else {
            std::cerr << "unknown argument: " << arg << "\n";
            usage();
            return 2;
        }
    }

    std::signal(SIGINT, handleSignal);
    std::signal(SIGTERM, handleSignal);

    try {
        BridgeState state;
        state.config = loadConfig(configPath);
        if(forceFake) {
            state.config.daphneFake = true;
            state.config.powerFake = true;
        }
        for(const auto &id : state.config.daphneIds) {
            DaphneRuntime board;
            board.id = id;
            board.endpoint = state.config.daphneEndpoints.at(id);
            board.client = std::make_unique<DaphneClient>(
                state.daphneContext,
                state.config.daphneEnabled,
                state.config.daphneFake,
                board.id,
                board.endpoint,
                state.config.daphneRoutes.at(id),
                state.config.daphneTimeoutMs);
            state.daphnes.push_back(std::move(board));
        }
        state.power = std::make_unique<ScpiPowerSupply>(state.config);

        const int port = parseEndpointPort(state.config.opcuaEndpoint);
        UA_Server *server = UA_Server_new();
        UA_ServerConfig *serverConfig = UA_Server_getConfig(server);
        UA_StatusCode rc = UA_ServerConfig_setMinimal(serverConfig, static_cast<UA_UInt16>(port), nullptr);
        if(rc != UA_STATUSCODE_GOOD)
            throw std::runtime_error("UA_ServerConfig_setMinimal failed");
        configureAccessControl(serverConfig, state);

        buildNamespace(server, state);
        updateNodes(server, state);
        rc = UA_Server_addRepeatedCallback(server, pollCallback, &state,
                                           static_cast<UA_Double>(state.config.pollPeriodMs), nullptr);
        if(rc != UA_STATUSCODE_GOOD)
            throw std::runtime_error("UA_Server_addRepeatedCallback failed");

        std::cout << "PDS OPC-UA bridge listening on port " << port
                  << " endpoint=" << state.config.opcuaEndpoint << "\n";
        std::cout << "DAPHNE boards=" << state.daphnes.size()
                  << (state.config.daphneFake ? " (fake)" : "")
                  << "; power " << (state.config.powerFake ? "fake" : state.config.powerDevice) << "\n";
        std::cout << "OPC-UA control policy="
                  << (state.config.controlPolicyPath.empty() ? "<none>" : state.config.controlPolicyPath)
                  << " operations=" << state.config.controlPolicy.size()
                  << " writes=" << (state.config.daphneWritesEnabled ? "enabled" : "disabled")
                  << "\n";
        const auto registrySummary = pds::registry::summarize(
            state.config.registryEntries, state.config.registryNodes);
        std::cout << "OPC-UA registry="
                  << (state.config.registryTagListPath.empty()
                          ? "<none>"
                          : state.config.registryTagListPath)
                  << " patterns=" << registrySummary.patterns
                  << " read_nodes=" << registrySummary.expandedReadNodes
                  << " method_nodes=" << registrySummary.expandedMethodNodes
                  << " namespace_version=" << state.config.registryNamespaceVersion
                  << "\n";
        const size_t printedBoards = std::min<size_t>(state.daphnes.size(), 10);
        for(size_t index = 0; index < printedBoards; ++index) {
            const auto &board = state.daphnes[index];
            std::cout << "  DAPHNE " << board.id << " endpoint=" << board.endpoint << "\n";
        }
        if(state.daphnes.size() > printedBoards)
            std::cout << "  ... " << state.daphnes.size() - printedBoards
                      << " additional DAPHNE endpoints\n";

        rc = UA_Server_run_startup(server);
        if(rc != UA_STATUSCODE_GOOD)
            throw std::runtime_error("UA_Server_run_startup failed");
        startPollers(state);
        while(g_running.load())
            (void)UA_Server_run_iterate(server, true);
        stopPollers(state);
        rc = UA_Server_run_shutdown(server);
        UA_Server_delete(server);
        google::protobuf::ShutdownProtobufLibrary();
        return rc == UA_STATUSCODE_GOOD ? 0 : 1;
    } catch(const std::exception &err) {
        std::cerr << "fatal: " << err.what() << "\n";
        google::protobuf::ShutdownProtobufLibrary();
        return 1;
    }
}
