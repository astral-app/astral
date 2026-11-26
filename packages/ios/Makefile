THEOS ?= $(HOME)/theos
TARGET := iphone:clang:latest:18.0
ARCHS := arm64
THEOS_PACKAGE_SCHEME := rootless

ROOT := $(abspath ../..)
ASTRAL_VERSION := $(shell cat $(ROOT)/version.txt)
PACKAGE_VERSION := $(ASTRAL_VERSION)
SIGNING_KEY := $(shell cat $(ROOT)/keys/plugin-signing.pub)
CORE := $(abspath ../core)
LUA := $(ROOT)/vendor/lua
LUA_SOURCES := lapi lcode lctype ldebug ldo ldump lfunc lgc llex lmem lobject lopcodes lparser lstate lstring ltable ltm lundump lvm lzio lauxlib lbaselib lstrlib ltablib lmathlib lutf8lib lcorolib
ANDROID_ONLY_SOURCES := AstralSettingsNavigator
CORE_SOURCES := $(filter-out $(ANDROID_ONLY_SOURCES),$(basename $(notdir $(wildcard $(CORE)/*.cpp))))

GENERATED := $(abspath .theos/generated)
RUST_TARGET := aarch64-apple-ios
RUST_LIBRARY := $(CORE)/target/$(RUST_TARGET)/release/libastral_core.a

include $(THEOS)/makefiles/common.mk

TWEAK_NAME := astral
astral_FILES := $(wildcard sources/*.mm sources/*.m sources/*.c sources/*.S sources/*.cpp) $(addprefix $(CORE)/,$(addsuffix .cpp,$(CORE_SOURCES))) $(addprefix $(LUA)/,$(addsuffix .c,$(LUA_SOURCES)))
ASTRAL_COMMIT ?= $(shell git -C $(ROOT) rev-parse --short=9 HEAD 2>/dev/null || echo unknown)
astral_CFLAGS := -fobjc-arc -flto=thin -Isources -I$(CORE)/wire -I$(GENERATED) -I$(CORE)/include -isystem $(ROOT)/vendor/json/single_include -I$(LUA) -DLUA_USE_POSIX -DASTRAL_VERSION=\"$(ASTRAL_VERSION)\" -DASTRAL_COMMIT=\"$(ASTRAL_COMMIT)\" -DASTRAL_SIGNING_KEY=\"$(SIGNING_KEY)\" -isystem $(ROOT)/vendor/tomlplusplus/include
astral_CCFLAGS := -std=c++20 -fno-cxx-modules
astral_LDFLAGS := -flto=thin $(RUST_LIBRARY)
astral_FRAMEWORKS := Foundation UIKit Photos UserNotifications AVFoundation Security MetricKit UniformTypeIdentifiers ImageIO AuthenticationServices SafariServices WebKit
astral_LIBRARIES := sqlite3

include $(THEOS_MAKE_PATH)/tweak.mk

$(THEOS_OBJ_DIR)/$(TWEAK_NAME).dylib: $(RUST_LIBRARY)

before-all::
	IPHONEOS_DEPLOYMENT_TARGET=18.0 cargo build --release --quiet --target $(RUST_TARGET) --manifest-path "$(CORE)/Cargo.toml"

before-package::
	cd $(ROOT) && go run ./packages/tools/cmd/astral stage "$(THEOS_STAGING_DIR)/usr/lib/astral"
