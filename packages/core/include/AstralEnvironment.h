#pragma once

#include <cstddef>
#include <cstdint>
#include <functional>
#include <optional>
#include <string>
#include <vector>

#include "AstralRust.h"

extern "C" {
typedef struct {
    const char *teamIdentifier;
    size_t teamIdentifierLength;
    int getTaskAllow;
    int hasProfile;
    int provisionsAllDevices;
    int listsDevices;
    const char *profileExpiry;
    size_t profileExpiryLength;
    const char *teamName;
    size_t teamNameLength;
    const char *sideloader;
    size_t sideloaderLength;
} AstralSigningFacts;

int astral_environment_receipt_kind(const char *name, size_t length, int exists);
int astral_environment_is_troll_store_install(int receipt, int trollStore, int trollStoreLite);
AstralRustString astral_environment_troll_store_variant(int trollStore, int trollStoreLite);
AstralRustString astral_environment_app_source(int simulator, int receipt, int trollStore, int trollStoreLite, int liveContainer, int jit,
                                               int discordSigned);
AstralRustString astral_environment_sideloader_name(const char *urlSchemes, size_t urlSchemesLength, const char *exportedTypes,
                                                    size_t exportedTypesLength, int altBundleIdentifier, int altDeviceIdentifier);
AstralRustString astral_environment_signing_details(const AstralSigningFacts *facts);
AstralRustString astral_environment_provisioning_profile_plist(const char *profile, size_t length);
int astral_environment_is_discord_production_team(const char *team, size_t length);
int astral_environment_is_virtual_device_machine(const char *machine, size_t length);
AstralRustString astral_environment_os_version_label(const char *version, size_t versionLength, const char *build, size_t buildLength);
AstralRustString astral_environment_default_ipv4_gateway(const uint8_t *table, size_t length);
AstralRustString astral_environment_hook_framework_for_image(const char *path, size_t length, int libhooker);
AstralRustString astral_environment_dpkg_package_version(const char *status, size_t statusLength, const char *package,
                                                         size_t packageLength);
AstralRustString astral_environment_framework_label(const char *name, size_t nameLength, const char *version, size_t versionLength);
AstralRustString astral_environment_jailbreak_name(const char *root, size_t rootLength, const char *markers, size_t markersLength,
                                                   const char *images, size_t imagesLength, int roothide);
AstralRustString astral_environment_jailbreak_label(const char *name, size_t nameLength, const char *version, size_t versionLength);
AstralRustString astral_environment_bootstrap_root_of_image(const char *path, size_t length);
}

namespace astral::environment {
enum class Receipt {
    None,
    AppStore,
    TestFlight,
};

struct InstallFacts {
    bool simulator = false;
    Receipt receipt = Receipt::None;
    bool trollStore = false;
    bool trollStoreLite = false;
    bool liveContainer = false;
    std::function<bool()> jitAvailable;
    std::optional<bool> discordSigned;
};

struct SigningFacts {
    std::optional<std::string> teamIdentifier;
    bool getTaskAllow = false;
    bool hasProfile = false;
    bool provisionsAllDevices = false;
    bool listsDevices = false;
    std::optional<std::string> profileExpiry;
    std::optional<std::string> teamName;
    std::string sideloader;
    std::string entitlementSource;
};

constexpr const char *kSandboxReceiptName = "sandboxReceipt";
constexpr const char *kTrollStoreMarker = "../_TrollStore";
constexpr const char *kTrollStoreLiteMarker = "../_TrollStoreLite";
constexpr const char *kDiscordTeamIdentifier = "53Q6R32WPB";
constexpr const char *kVirtualDeviceMachine = "iPhone99,11";

struct SideloaderMarkers {
    std::vector<std::string> urlSchemes;
    std::vector<std::string> exportedTypes;
    bool altBundleIdentifier = false;
    bool altDeviceIdentifier = false;
};

struct HookFramework {
    std::string name;
    std::string package;
};

inline std::string joinedEntries(const std::vector<std::string> &entries)
{
    std::string joined;
    for (const auto &entry : entries) {
        joined += entry;
        joined.push_back('\0');
    }
    return joined;
}

inline const char *optionalData(const std::optional<std::string> &value)
{
    return value ? value->data() : nullptr;
}

inline size_t optionalSize(const std::optional<std::string> &value)
{
    return value ? value->size() : 0;
}

inline std::string sideloaderName(const SideloaderMarkers &markers)
{
    std::string schemes = joinedEntries(markers.urlSchemes), types = joinedEntries(markers.exportedTypes);
    return rust::take(astral_environment_sideloader_name(schemes.data(), schemes.size(), types.data(), types.size(),
                                                         markers.altBundleIdentifier, markers.altDeviceIdentifier));
}

inline Receipt receiptKind(const std::optional<std::string> &receiptName, bool receiptExists)
{
    return static_cast<Receipt>(astral_environment_receipt_kind(optionalData(receiptName), optionalSize(receiptName), receiptExists));
}

inline bool isTrollStoreInstall(Receipt receipt, bool trollStore, bool trollStoreLite)
{
    return astral_environment_is_troll_store_install(static_cast<int>(receipt), trollStore, trollStoreLite);
}

inline std::string trollStoreVariant(bool trollStore, bool trollStoreLite)
{
    return rust::take(astral_environment_troll_store_variant(trollStore, trollStoreLite));
}

inline std::string appSource(const InstallFacts &facts)
{
    bool reachesLiveContainer =
        facts.liveContainer && !facts.simulator && !isTrollStoreInstall(facts.receipt, facts.trollStore, facts.trollStoreLite);
    bool jit = reachesLiveContainer && facts.jitAvailable && facts.jitAvailable();
    int signedState = facts.discordSigned ? (*facts.discordSigned ? 1 : 0) : -1;
    return rust::take(astral_environment_app_source(facts.simulator, static_cast<int>(facts.receipt), facts.trollStore,
                                                    facts.trollStoreLite, facts.liveContainer, jit, signedState));
}

inline std::string signingDetails(const SigningFacts &facts)
{
    AstralSigningFacts raw = {optionalData(facts.teamIdentifier),
                              optionalSize(facts.teamIdentifier),
                              facts.getTaskAllow,
                              facts.hasProfile,
                              facts.provisionsAllDevices,
                              facts.listsDevices,
                              optionalData(facts.profileExpiry),
                              optionalSize(facts.profileExpiry),
                              optionalData(facts.teamName),
                              optionalSize(facts.teamName),
                              facts.sideloader.data(),
                              facts.sideloader.size()};
    return rust::take(astral_environment_signing_details(&raw));
}

inline std::optional<std::string> provisioningProfilePlist(const std::string &profile)
{
    return rust::takeIfPresent(astral_environment_provisioning_profile_plist(profile.data(), profile.size()));
}

inline bool isDiscordProductionTeam(const std::optional<std::string> &teamIdentifier)
{
    return astral_environment_is_discord_production_team(optionalData(teamIdentifier), optionalSize(teamIdentifier));
}

inline bool isVirtualDeviceMachine(const std::string &machine)
{
    return astral_environment_is_virtual_device_machine(machine.data(), machine.size());
}

inline std::string osVersionLabel(const std::string &version, const std::optional<std::string> &build)
{
    return rust::take(astral_environment_os_version_label(version.data(), version.size(), optionalData(build), optionalSize(build)));
}

inline std::optional<std::string> defaultIPv4Gateway(const uint8_t *table, size_t length)
{
    return rust::takeIfPresent(astral_environment_default_ipv4_gateway(table, length));
}

inline std::optional<HookFramework> hookFrameworkForImage(const std::string &imagePath, bool exportsLibhookerAPI)
{
    auto string = astral_environment_hook_framework_for_image(imagePath.data(), imagePath.size(), exportsLibhookerAPI);
    if (!string.bytes) return std::nullopt;
    auto parts = rust::takeFrames(string);
    if (parts.size() != 2) return std::nullopt;
    return HookFramework{parts[0], parts[1]};
}

inline std::optional<std::string> dpkgPackageVersion(const std::string &status, const std::string &package)
{
    return rust::takeIfPresent(astral_environment_dpkg_package_version(status.data(), status.size(), package.data(), package.size()));
}

inline std::string frameworkLabel(const std::string &name, const std::string &version)
{
    return rust::take(astral_environment_framework_label(name.data(), name.size(), version.data(), version.size()));
}

inline std::string jailbreakName(const std::string &root, const std::vector<std::string> &markers, const std::vector<std::string> &images,
                                 bool roothide = false)
{
    std::string markerList = joinedEntries(markers), imageList = joinedEntries(images);
    return rust::take(astral_environment_jailbreak_name(root.data(), root.size(), markerList.data(), markerList.size(), imageList.data(),
                                                        imageList.size(), roothide));
}

inline std::string jailbreakLabel(const std::string &name, const std::string &version)
{
    return rust::take(astral_environment_jailbreak_label(name.data(), name.size(), version.data(), version.size()));
}

inline std::optional<std::string> bootstrapRootOfImage(const std::string &imagePath)
{
    return rust::takeIfPresent(astral_environment_bootstrap_root_of_image(imagePath.data(), imagePath.size()));
}
} // namespace astral::environment
