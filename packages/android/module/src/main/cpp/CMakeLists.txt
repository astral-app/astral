cmake_minimum_required(VERSION 3.22.1)
project(astral C CXX)

set(CORE ${ASTRAL_ROOT}/packages/core)
set(VENDOR ${ASTRAL_ROOT}/vendor)
file(STRINGS ${ASTRAL_ROOT}/keys/plugin-signing.pub SIGNING_KEY LIMIT_COUNT 1)
set(LUA_SOURCES lapi lcode lctype ldebug ldo ldump lfunc lgc llex lmem lobject lopcodes lparser lstate lstring ltable ltm lundump lvm lzio lauxlib lbaselib lstrlib ltablib lmathlib lutf8lib lcorolib)

set(ASTRAL_GENERATED ${CMAKE_CURRENT_BINARY_DIR}/generated)
set(PRELUDE ${CORE}/lua/prelude.lua)
set_property(DIRECTORY APPEND PROPERTY CMAKE_CONFIGURE_DEPENDS ${PRELUDE})
execute_process(COMMAND go run ./packages/tools/cmd/astral embed-prelude ${PRELUDE} ${ASTRAL_GENERATED}/AstralPrelude.h
                WORKING_DIRECTORY ${ASTRAL_ROOT} RESULT_VARIABLE PRELUDE_RESULT)
if(NOT PRELUDE_RESULT EQUAL 0)
    message(FATAL_ERROR "astral embed-prelude failed")
endif()

file(GLOB CORE_SOURCES CONFIGURE_DEPENDS ${CORE}/src/*.cpp)
list(FILTER CORE_SOURCES EXCLUDE REGEX "AstralMetricKitCrash\\.cpp$")
list(TRANSFORM LUA_SOURCES PREPEND ${VENDOR}/lua/)
list(TRANSFORM LUA_SOURCES APPEND .c)

if(ANDROID_ABI STREQUAL "arm64-v8a")
    set(RUST_TARGET aarch64-linux-android)
elseif(ANDROID_ABI STREQUAL "x86_64")
    set(RUST_TARGET x86_64-linux-android)
else()
    message(FATAL_ERROR "astral core has no Rust target for ${ANDROID_ABI}")
endif()
find_program(CARGO cargo HINTS $ENV{HOME}/.cargo/bin REQUIRED)
set(RUST_CRATE ${CORE})
set(RUST_LIBRARY ${RUST_CRATE}/target/${RUST_TARGET}/release/libastral_core.a)
file(GLOB_RECURSE RUST_SOURCES CONFIGURE_DEPENDS ${RUST_CRATE}/src/*.rs)
add_custom_command(OUTPUT ${RUST_LIBRARY}
                   COMMAND ${CARGO} build --release --quiet --target ${RUST_TARGET} --manifest-path ${RUST_CRATE}/Cargo.toml
                   DEPENDS ${RUST_SOURCES} ${RUST_CRATE}/Cargo.toml
                   WORKING_DIRECTORY ${RUST_CRATE})
add_custom_target(astral-core-rust DEPENDS ${RUST_LIBRARY})

add_library(astral-vendor STATIC ${LUA_SOURCES} ${VENDOR}/sqlite/sqlite3.c)
target_compile_definitions(astral-vendor PRIVATE LUA_USE_POSIX SQLITE_THREADSAFE=2 SQLITE_OMIT_LOAD_EXTENSION)
target_compile_options(astral-vendor PRIVATE -O2 -w -fvisibility=hidden)

add_library(astral SHARED AstralJni.cpp AstralAndroidBackup.cpp AstralAndroidMarketplace.cpp ${CORE_SOURCES})
target_include_directories(astral PRIVATE ${CORE}/include ${CORE}/wire ${ASTRAL_GENERATED} ${VENDOR}/lua ${VENDOR}/sqlite)
target_include_directories(astral SYSTEM PRIVATE ${VENDOR}/json/single_include ${VENDOR}/tomlplusplus/include)
target_compile_definitions(astral PRIVATE ASTRAL_VERSION="${ASTRAL_VERSION}" ASTRAL_COMMIT="${ASTRAL_COMMIT}")
target_compile_definitions(astral PRIVATE ASTRAL_SIGNING_KEY="${SIGNING_KEY}")
target_compile_options(astral PRIVATE -O2 -Wall -fvisibility=hidden)
add_dependencies(astral astral-core-rust)
target_link_libraries(astral ${RUST_LIBRARY} astral-vendor log m)
target_link_options(astral PRIVATE -Wl,-z,max-page-size=16384 -Wl,--exclude-libs,ALL)
