THEOS ?= $(HOME)/theos
TARGET := iphone:clang:latest:18.0
ARCHS := arm64 arm64e
THEOS_PACKAGE_SCHEME := rootless

ROOT := ../..
ASTRAL_VERSION := $(shell cat $(ROOT)/version.txt)
PACKAGE_VERSION := $(ASTRAL_VERSION)
SIGNING_KEY := $(shell cat $(ROOT)/keys/plugin-signing.pub)
MONOCYPHER := $(ROOT)/vendor/monocypher/src
CORE := ../core
LUA := $(ROOT)/vendor/lua
LUA_SOURCES := lapi lcode lctype ldebug ldo ldump lfunc lgc llex lmem lobject lopcodes lparser lstate lstring ltable ltm lundump lvm lzio lauxlib lbaselib lstrlib ltablib lmathlib lutf8lib lcorolib
CORE_SOURCES := LoaderSettings LoaderSettingsCache LoaderSettingsFile LoaderFiles LoaderJSON LoaderJSONMembers LoaderDiskFull LoaderLog LoaderExitRecord LoaderLaunchGuard LoaderStrings

include $(THEOS)/makefiles/common.mk

TWEAK_NAME := Astral
Astral_FILES := $(wildcard sources/*.mm sources/*.m sources/*.c sources/*.S sources/*.cpp) $(addprefix $(CORE)/src/,$(addsuffix .cpp,$(CORE_SOURCES))) $(addprefix $(LUA)/,$(addsuffix .c,$(LUA_SOURCES))) $(MONOCYPHER)/monocypher.c $(MONOCYPHER)/optional/monocypher-ed25519.c
Astral_CFLAGS := -fobjc-arc -Isources -I$(CORE)/include -I$(ROOT)/vendor -I$(LUA) -DLUA_USE_POSIX -DASTRAL_VERSION=\"$(ASTRAL_VERSION)\" -DASTRAL_SIGNING_KEY=\"$(SIGNING_KEY)\" -I$(MONOCYPHER) -I$(MONOCYPHER)/optional -isystem $(ROOT)/vendor/tomlplusplus/include
Astral_CCFLAGS := -std=c++20 -fno-cxx-modules
Astral_FRAMEWORKS := Foundation UIKit Security
Astral_LIBRARIES := substrate sqlite3

include $(THEOS_MAKE_PATH)/tweak.mk

before-package::
	$(ROOT)/tools/stage-resources.sh "$(THEOS_STAGING_DIR)/usr/lib/astral"
