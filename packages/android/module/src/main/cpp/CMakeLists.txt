cmake_minimum_required(VERSION 3.22.1)
project(astral C CXX)

set(WIRE ${ASTRAL_ROOT}/packages/core/wire)
file(GLOB WIRE_SOURCES ${WIRE}/*.c)

add_library(astral SHARED AstralJni.c ${WIRE_SOURCES})
target_include_directories(astral PRIVATE ${WIRE})
target_compile_definitions(astral PRIVATE ASTRAL_VERSION="${ASTRAL_VERSION}")
target_compile_options(astral PRIVATE -O2 -Wall -Werror -fvisibility=hidden)
target_link_libraries(astral log m)
target_link_options(astral PRIVATE -Wl,-z,max-page-size=16384)
