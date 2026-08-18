cmake_minimum_required(VERSION 3.20)

project(pds_opcua_bridge LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

find_package(open62541 REQUIRED)
find_package(Protobuf REQUIRED)
find_package(PkgConfig REQUIRED)
pkg_check_modules(LIBZMQ REQUIRED IMPORTED_TARGET libzmq)
find_path(CPPZMQ_INCLUDE_DIR zmq.hpp)
if(NOT CPPZMQ_INCLUDE_DIR)
    message(FATAL_ERROR "cppzmq header zmq.hpp not found")
endif()

add_library(pds_registry STATIC src/registry.cpp)
target_include_directories(pds_registry PUBLIC "${CMAKE_CURRENT_LIST_DIR}/src")
target_compile_options(pds_registry PRIVATE -Wall -Wextra -Wpedantic)

set(REPO_ROOT "${CMAKE_CURRENT_LIST_DIR}/..")
set(PROTO_OUT_DIR "${CMAKE_CURRENT_BINARY_DIR}/generated")
file(MAKE_DIRECTORY "${PROTO_OUT_DIR}")

set(PROTO_FILES
    "${REPO_ROOT}/proto/upstream/daphneV3_high_level_confs.proto"
    "${REPO_ROOT}/proto/upstream/daphneV3_low_level_confs.proto"
    "${REPO_ROOT}/proto/upstream/daphne_v8_telemetry.proto"
)

set(GENERATED_PROTO_SRCS)
set(GENERATED_PROTO_HDRS)
foreach(PROTO_FILE IN LISTS PROTO_FILES)
    get_filename_component(PROTO_STEM "${PROTO_FILE}" NAME_WE)
    list(APPEND GENERATED_PROTO_SRCS "${PROTO_OUT_DIR}/${PROTO_STEM}.pb.cc")
    list(APPEND GENERATED_PROTO_HDRS "${PROTO_OUT_DIR}/${PROTO_STEM}.pb.h")
    add_custom_command(
        OUTPUT "${PROTO_OUT_DIR}/${PROTO_STEM}.pb.cc" "${PROTO_OUT_DIR}/${PROTO_STEM}.pb.h"
        COMMAND protobuf::protoc
            --cpp_out=${PROTO_OUT_DIR}
            -I "${REPO_ROOT}/proto/upstream"
            -I "${REPO_ROOT}/proto"
            "${PROTO_FILE}"
        DEPENDS "${PROTO_FILE}"
        VERBATIM
    )
endforeach()

add_executable(pds-opcua-bridge
    src/daphne_client.cpp
    src/main.cpp
    src/opcua_snapshot_writer.cpp
    src/v8_snapshot.cpp
    ${GENERATED_PROTO_SRCS}
    ${GENERATED_PROTO_HDRS}
)

target_include_directories(pds-opcua-bridge PRIVATE "${PROTO_OUT_DIR}" "${CPPZMQ_INCLUDE_DIR}")
target_link_libraries(pds-opcua-bridge
    PRIVATE
        pds_registry
        open62541::open62541
        protobuf::libprotobuf
        PkgConfig::LIBZMQ
)

target_compile_options(pds-opcua-bridge PRIVATE -Wall -Wextra -Wpedantic)

add_executable(pds-registry-check tools/registry_check.cpp)
target_link_libraries(pds-registry-check PRIVATE pds_registry)
target_compile_options(pds-registry-check PRIVATE -Wall -Wextra -Wpedantic)

add_executable(pds-opcua-registry-smoke tools/opcua_registry_smoke.cpp)
target_link_libraries(pds-opcua-registry-smoke PRIVATE pds_registry open62541::open62541)
target_compile_options(pds-opcua-registry-smoke PRIVATE -Wall -Wextra -Wpedantic)

add_executable(pds-opcua-read-nodes tools/opcua_read_nodes.cpp)
target_link_libraries(pds-opcua-read-nodes PRIVATE open62541::open62541)
target_compile_options(pds-opcua-read-nodes PRIVATE -Wall -Wextra -Wpedantic)

install(TARGETS pds-opcua-bridge pds-registry-check pds-opcua-registry-smoke
        RUNTIME DESTINATION bin)

include(CTest)
if(BUILD_TESTING)
    add_executable(pds-v8-snapshot-test
        tests/v8_snapshot_test.cpp
        src/v8_snapshot.cpp
        "${PROTO_OUT_DIR}/daphne_v8_telemetry.pb.cc"
        "${PROTO_OUT_DIR}/daphne_v8_telemetry.pb.h"
    )
    target_include_directories(pds-v8-snapshot-test PRIVATE
        "${CMAKE_CURRENT_LIST_DIR}/src" "${PROTO_OUT_DIR}")
    target_link_libraries(pds-v8-snapshot-test PRIVATE protobuf::libprotobuf)
    target_compile_options(pds-v8-snapshot-test PRIVATE -Wall -Wextra -Wpedantic)
    add_test(NAME daphne-v8-snapshot-validation COMMAND pds-v8-snapshot-test)
endif()

set(DAPHNE_TAG_LIST "" CACHE FILEPATH "Path to the exported DAPHNE tag_list.csv")
if(BUILD_TESTING AND DAPHNE_TAG_LIST)
    add_test(NAME daphne-registry-contract
             COMMAND pds-registry-check "${DAPHNE_TAG_LIST}" 383 362 21)
endif()
