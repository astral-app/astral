THEOS ?= $(HOME)/theos
TARGET := iphone:clang:latest:18.0
ARCHS := arm64 arm64e
THEOS_PACKAGE_SCHEME := rootless

ROOT := $(abspath ../..)
ASTRAL_VERSION := $(shell cat $(ROOT)/version.txt)
PACKAGE_VERSION := $(ASTRAL_VERSION)
SIGNING_KEY := $(shell cat $(ROOT)/keys/plugin-signing.pub)
MONOCYPHER := $(ROOT)/vendor/monocypher/src
CORE := $(abspath ../core)
LUA := $(ROOT)/vendor/lua
LUA_SOURCES := lapi lcode lctype ldebug ldo ldump lfunc lgc llex lmem lobject lopcodes lparser lstate lstring ltable ltm lundump lvm lzio lauxlib lbaselib lstrlib ltablib lmathlib lutf8lib lcorolib
CORE_SOURCES := AstralSignatures AstralThemes AstralPacks AstralTOML AstralCutout AstralLuaHost AstralSettingsPages AstralDiscordCache AstralDevServer AstralReactors AstralSession AstralRequestSink AstralMetricKitCrash AstralNetworkPolicy AstralSettings AstralSettingsCache AstralSettingsFile AstralFiles AstralJSON AstralJSONMembers AstralDiskFull AstralLog AstralExitRecord AstralLaunchGuard AstralStrings AstralSettingsRecovery LuaSandbox LuaJSON LuaLocale PluginPackage PluginStorage

GENERATED := $(abspath .theos/generated)
PRELUDE := $(CORE)/lua/prelude.lua

include $(THEOS)/makefiles/common.mk

TWEAK_NAME := astral
astral_FILES := $(wildcard sources/*.mm sources/*.m sources/*.c sources/*.S sources/*.cpp $(CORE)/wire/*.c) $(addprefix $(CORE)/src/,$(addsuffix .cpp,$(CORE_SOURCES))) $(addprefix $(LUA)/,$(addsuffix .c,$(LUA_SOURCES))) $(MONOCYPHER)/monocypher.c $(MONOCYPHER)/optional/monocypher-ed25519.c
astral_CFLAGS := -fobjc-arc -flto=thin -Isources -I$(CORE)/wire -I$(GENERATED) -I$(CORE)/include -isystem $(ROOT)/vendor/json/single_include -I$(LUA) -DLUA_USE_POSIX -DASTRAL_VERSION=\"$(ASTRAL_VERSION)\" -DASTRAL_SIGNING_KEY=\"$(SIGNING_KEY)\" -I$(MONOCYPHER) -I$(MONOCYPHER)/optional -isystem $(ROOT)/vendor/tomlplusplus/include
astral_CCFLAGS := -std=c++20 -fno-cxx-modules
astral_LDFLAGS := -flto=thin
astral_FRAMEWORKS := Foundation UIKit Security MetricKit UniformTypeIdentifiers ImageIO
astral_LIBRARIES := sqlite3

include $(THEOS_MAKE_PATH)/tweak.mk

before-all::
	cd $(ROOT) && go run ./packages/tools/cmd/astral embed-prelude "$(PRELUDE)" "$(GENERATED)/AstralPrelude.h"

before-package::
	cd $(ROOT) && go run ./packages/tools/cmd/astral stage "$(THEOS_STAGING_DIR)/usr/lib/astral"
