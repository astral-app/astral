#pragma once

#include <nlohmann/json.hpp>
#include <optional>
#include <string>
#include <vector>

#include "AstralRust.h"

extern "C" {
int astral_env_is_beta_build(const char *build, size_t length);
AstralRustString astral_env_apple_markers(const char *facts, size_t length);
AstralRustString astral_env_android_markers(const char *facts, size_t length);
AstralRustString astral_env_locale_markers(const char *facts, size_t length);
AstralRustString astral_env_display_markers(int64_t refreshRate, int boldText);
AstralRustString astral_env_duplicate_copy_marker(const char *path, size_t length);
AstralRustString astral_env_https_verdict(const char *check, size_t length);
AstralRustString astral_env_android_install_line(const char *facts, size_t length);
AstralRustString astral_env_crash_summary(const char *reports, size_t length, double now);
void astral_env_record_https_check(const char *check, size_t length);
AstralRustString astral_env_current_https_check(void);
}

namespace astral::environment_report {

struct LocaleFacts {
    std::string calendar;
    bool nonLatinDigits = false;
    bool rightToLeft = false;
};

struct AppleDeviceFacts {
    std::string modelEdit;
    std::string jailbreakVersion;
    bool iOSAppOnVision = false;
    bool sideStoreVPN = false;
    bool liveContainerMultitask = false;
    int refreshRate = 0;
    bool boldText = false;
    LocaleFacts locale;
    bool lockdownMode = false;
    bool iPhoneAppOnIPad = false;
    bool iOSAppOnMac = false;
    bool playCover = false;
    bool managedApp = false;
    bool jitEnabled = false;
    bool debuggerAttached = false;
    bool trollFools = false;
    bool appSync = false;
    std::string duplicateCopy;
};

struct AndroidDeviceFacts {
    std::optional<bool> selinuxEnforcing;
    std::string verifiedBootState;
    std::string buildTags;
    std::string customROM;
    long pageSize = 0;
    std::string webView;
    std::string host;
    std::string skin;
    bool batteryOptimized = false;
    bool backgroundRestricted = false;
    int standbyBucket = 0;
    std::string abiBridge;
    bool lowRam = false;
    bool harmonyOS = false;
    bool adoptableStorage = false;
    int refreshRate = 0;
    bool boldText = false;
    std::string formFactor;
    std::string duplicateCopy;
    LocaleFacts locale;
};

struct HttpsCheck {
    bool completed = false;
    std::vector<std::string> issuers;
    std::string failure;
};

inline nlohmann::json localeJSON(const LocaleFacts &facts)
{
    return {{"calendar", facts.calendar}, {"nonLatinDigits", facts.nonLatinDigits}, {"rightToLeft", facts.rightToLeft}};
}

inline nlohmann::json httpsJSON(const HttpsCheck &check)
{
    return {{"completed", check.completed}, {"issuers", check.issuers}, {"failure", check.failure}};
}

inline bool isBetaBuild(const std::string &build)
{
    return astral_env_is_beta_build(build.data(), build.size()) != 0;
}

inline std::vector<std::string> localeMarkers(const LocaleFacts &facts)
{
    std::string text = localeJSON(facts).dump();
    return rust::takeList(astral_env_locale_markers(text.data(), text.size()));
}

inline std::vector<std::string> displayMarkers(int refreshRate, bool boldText)
{
    return rust::takeList(astral_env_display_markers(refreshRate, boldText));
}

inline std::string duplicateCopyMarker(const std::string &path)
{
    return rust::take(astral_env_duplicate_copy_marker(path.data(), path.size()));
}

inline std::vector<std::string> appleMarkers(const AppleDeviceFacts &facts)
{
    std::string text = nlohmann::json{{"modelEdit", facts.modelEdit},
                                      {"jailbreakVersion", facts.jailbreakVersion},
                                      {"iOSAppOnVision", facts.iOSAppOnVision},
                                      {"sideStoreVPN", facts.sideStoreVPN},
                                      {"liveContainerMultitask", facts.liveContainerMultitask},
                                      {"refreshRate", facts.refreshRate},
                                      {"boldText", facts.boldText},
                                      {"locale", localeJSON(facts.locale)},
                                      {"lockdownMode", facts.lockdownMode},
                                      {"iPhoneAppOnIPad", facts.iPhoneAppOnIPad},
                                      {"iOSAppOnMac", facts.iOSAppOnMac},
                                      {"playCover", facts.playCover},
                                      {"managedApp", facts.managedApp},
                                      {"jitEnabled", facts.jitEnabled},
                                      {"debuggerAttached", facts.debuggerAttached},
                                      {"trollFools", facts.trollFools},
                                      {"appSync", facts.appSync},
                                      {"duplicateCopy", facts.duplicateCopy}}
                           .dump(-1, ' ', false, nlohmann::json::error_handler_t::replace);
    return rust::takeList(astral_env_apple_markers(text.data(), text.size()));
}

inline std::vector<std::string> androidMarkers(const AndroidDeviceFacts &facts)
{
    nlohmann::json json{{"verifiedBootState", facts.verifiedBootState},
                        {"buildTags", facts.buildTags},
                        {"customROM", facts.customROM},
                        {"pageSize", facts.pageSize},
                        {"webView", facts.webView},
                        {"host", facts.host},
                        {"skin", facts.skin},
                        {"batteryOptimized", facts.batteryOptimized},
                        {"backgroundRestricted", facts.backgroundRestricted},
                        {"standbyBucket", facts.standbyBucket},
                        {"abiBridge", facts.abiBridge},
                        {"lowRam", facts.lowRam},
                        {"harmonyOS", facts.harmonyOS},
                        {"adoptableStorage", facts.adoptableStorage},
                        {"refreshRate", facts.refreshRate},
                        {"boldText", facts.boldText},
                        {"formFactor", facts.formFactor},
                        {"duplicateCopy", facts.duplicateCopy},
                        {"locale", localeJSON(facts.locale)}};
    if (facts.selinuxEnforcing) json["selinuxEnforcing"] = *facts.selinuxEnforcing;
    std::string text = json.dump(-1, ' ', false, nlohmann::json::error_handler_t::replace);
    return rust::takeList(astral_env_android_markers(text.data(), text.size()));
}

inline nlohmann::json httpsVerdict(const HttpsCheck &check)
{
    std::string text = httpsJSON(check).dump(-1, ' ', false, nlohmann::json::error_handler_t::replace);
    auto verdict = nlohmann::json::parse(rust::take(astral_env_https_verdict(text.data(), text.size())), nullptr, false);
    return verdict.is_discarded() ? nlohmann::json() : verdict;
}

inline std::string androidInstallLine(const nlohmann::json &facts)
{
    std::string text = facts.dump(-1, ' ', false, nlohmann::json::error_handler_t::replace);
    return rust::take(astral_env_android_install_line(text.data(), text.size()));
}

inline nlohmann::json crashSummary(const nlohmann::json &reports, double now)
{
    std::string text = reports.dump(-1, ' ', false, nlohmann::json::error_handler_t::replace);
    auto summary = nlohmann::json::parse(rust::take(astral_env_crash_summary(text.data(), text.size(), now)), nullptr, false);
    return summary.is_discarded() ? nlohmann::json::object() : summary;
}

inline void recordHttpsCheck(HttpsCheck check)
{
    std::string text = httpsJSON(check).dump(-1, ' ', false, nlohmann::json::error_handler_t::replace);
    astral_env_record_https_check(text.data(), text.size());
}

inline HttpsCheck currentHttpsCheck()
{
    auto json = nlohmann::json::parse(rust::take(astral_env_current_https_check()), nullptr, false);
    HttpsCheck check;
    if (!json.is_object()) return check;
    check.completed = json.value("completed", false);
    check.failure = json.value("failure", "");
    for (const auto &issuer : json.value("issuers", nlohmann::json::array()))
        if (issuer.is_string()) check.issuers.push_back(issuer.get<std::string>());
    return check;
}
} // namespace astral::environment_report
