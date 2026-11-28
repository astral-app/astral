#![allow(clippy::missing_safety_doc)]

mod bridge;
#[cfg(target_vendor = "apple")]
mod facts;
mod network;
mod shapes;
mod text;

use astral_core::app::{self, Launch, pressable::Pressable, state::Platform};
use astral_core::lua::enums::Event;

pub use crate::bridge::{
    AstralBridge, AstralReply, AstralReplyKind, AstralRequest, AstralRequestKind,
};
pub use crate::network::{AstralMediaKind, AstralRequestPlan, AstralRequestProbe};
pub use crate::shapes::{AstralCutout, AstralCutoutKind, AstralRect};
pub use crate::text::{AstralOwned, AstralText};

#[repr(C)]
pub struct AstralLaunch {
    pub version: AstralText,
    pub commit: AstralText,
    pub state_directory: AstralText,
    pub discord_directory: AstralText,
    pub resource_directory: AstralText,
    pub discord_version: AstralText,
    pub languages: AstralText,
    pub environment: AstralText,
}

#[repr(u32)]
#[derive(Clone, Copy)]
pub enum AstralLifecycle {
    Foreground,
    Background,
}

impl AstralLifecycle {
    const fn event(self) -> Event {
        match self {
            AstralLifecycle::Foreground => Event::AppForeground,
            AstralLifecycle::Background => Event::AppBackground,
        }
    }
}

#[repr(u32)]
#[derive(Clone, Copy)]
pub enum AstralPressable {
    Ignored,
    Slider,
    Navigator,
    Inserted,
    MessageAction,
}

impl AstralPressable {
    const fn of(pressable: Pressable) -> Self {
        match pressable {
            Pressable::Ignored => AstralPressable::Ignored,
            Pressable::Slider => AstralPressable::Slider,
            Pressable::Navigator => AstralPressable::Navigator,
            Pressable::Inserted => AstralPressable::Inserted,
            Pressable::MessageAction => AstralPressable::MessageAction,
        }
    }
}

fn lines(text: AstralText) -> Vec<String> {
    text.text()
        .lines()
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_attach(bridge: AstralBridge) {
    bridge::attach(bridge);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_ios_prepare(launch: *const AstralLaunch) {
    let Some(launch) = (unsafe { launch.as_ref() }) else {
        return;
    };
    app::prepare(Launch {
        platform: Platform::Ios,
        version: launch.version.text(),
        commit: launch.commit.text(),
        property_names: Vec::new(),
        state_directory: launch.state_directory.text(),
        discord_directory: launch.discord_directory.text(),
        resource_directory: launch.resource_directory.text(),
        languages: lines(launch.languages),
        discord_version: launch.discord_version.text(),
        environment: serde_json::from_slice(launch.environment.bytes())
            .unwrap_or(serde_json::Value::Null),
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_start() -> usize {
    app::start()
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_mark_visible() {
    app::mark_visible();
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_mark_hidden() {
    app::mark_hidden();
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_mark_ready() {
    app::mark_ready();
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_emit_lifecycle(lifecycle: AstralLifecycle) {
    app::emit_event(lifecycle.event().value());
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_staff_dev_tools() -> bool {
    app::staff_dev_tools_enabled()
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_free(owned: AstralOwned) {
    owned.release();
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_plugins() -> AstralOwned {
    AstralOwned::text(app::plugins())
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_composer_styles(text: AstralText) -> AstralOwned {
    AstralOwned::text(app::composer_styles(&text.text()))
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_open_link(url: AstralText) -> bool {
    app::links::open(&url.text())
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_pressable(identifier: AstralText) -> AstralPressable {
    AstralPressable::of(app::pressable::pressable(&identifier.text()))
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_press(identifier: AstralText) -> bool {
    app::pressable::press(&identifier.text())
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_set_menu_message(channel: u64, message: u64) {
    app::set_menu_message(channel, message);
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_complete_network_request(request: u64, response: AstralText) {
    app::complete_network_request(request, &response.text());
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_sheet_event(
    sheet: u64,
    action: AstralText,
    value: AstralText,
    closed: bool,
) {
    app::sheet_event(sheet, action.text(), value.text(), closed);
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_notification_reply(
    plugin: AstralText,
    channel: AstralText,
    message: AstralText,
    text: AstralText,
) {
    app::notification_reply(plugin.text(), channel.text(), message.text(), text.text());
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_complete_confirm(request: u64, accepted: bool) {
    app::complete_confirm(request, accepted);
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_complete_web_auth(
    request: u64,
    url: AstralText,
    cookies: AstralText,
    error: AstralText,
) {
    app::complete_web_auth(
        request,
        url.optional(),
        cookies.optional(),
        error.optional(),
    );
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_complete_file_pick(
    request: u64,
    name: AstralText,
    data: AstralText,
    error: AstralText,
) {
    let failure = error.optional();
    app::complete_file_pick(
        request,
        name.present().then(|| name.bytes()),
        data.present().then(|| data.bytes()),
        failure.as_deref(),
    );
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_complete_text_edit(request: u64, value: AstralText) {
    app::complete_text_edit(request, value.optional());
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_page_category() -> AstralOwned {
    AstralOwned::text(app::page_category())
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_page_model(route: AstralText) -> AstralOwned {
    AstralOwned::text(app::page_model(&route.text()))
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_page_action(route: AstralText, key: AstralText) -> AstralOwned {
    AstralOwned::optional(app::page_action(&route.text(), &key.text()))
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_refresh_catalog(force: bool) {
    app::refresh_catalog(force);
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_restart_softly() -> usize {
    app::restart_softly()
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_icon_path(name: AstralText) -> AstralOwned {
    AstralOwned::text(app::icon_path(&name.text()))
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_font_faces() -> AstralOwned {
    AstralOwned::text(app::font_faces())
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_emoji_url(codepoints: AstralText) -> AstralOwned {
    AstralOwned::text(app::emoji_url(&codepoints.text()))
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_emoji_animated() -> bool {
    app::emoji_animated()
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_stop_list_reason(phoenix: AstralText) -> AstralOwned {
    AstralOwned::optional(app::stop_list_reason(&phoenix.text()))
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_developer_log(line: AstralText) {
    app::developer_log(&line.text());
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_pending_updates() -> AstralOwned {
    let updates = app::pending_updates();
    if updates.is_empty() {
        return AstralOwned::NONE;
    }
    AstralOwned::new(updates)
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_export_backup() -> AstralOwned {
    AstralOwned::text(app::export_backup())
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_apply_backup(contents: AstralText) -> AstralOwned {
    AstralOwned::optional(app::apply_backup(&contents.text()))
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_decorations_pending() -> bool {
    app::decorations_pending()
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_refresh_settings() {
    app::refresh_settings();
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_sync_chosen() {
    app::sync::chosen();
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_refresh_decorations() {
    app::refresh_decorations();
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_recovering() -> bool {
    app::state::app().is_some_and(|current| current.recovering())
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_custom_emoji_pack() -> bool {
    app::custom_emoji_pack()
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_record_diagnostics(payload: AstralText, images: AstralText) -> usize {
    app::record_diagnostics(&payload.text(), &images.text())
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_record_https_check(check: AstralText) {
    app::record_https_check(&check.text());
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_navigator_slide(identifier: AstralText, fraction: f64) {
    astral_core::settings::navigator::slide(&identifier.text(), fraction);
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_navigator_back() -> bool {
    astral_core::settings::navigator::back()
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_picked_media_fetched(entries: AstralText) {
    app::picked_media_fetched(&entries.text());
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_is_picked_video_url(url: AstralText) -> bool {
    app::picked_media::is_video_url(&url.text())
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_picked_video_file(url: AstralText) -> AstralOwned {
    match app::picked_media::video_file(&url.text()) {
        app::picked_media::VideoFile::Cached(file) => AstralOwned::text(file),
        _ => AstralOwned::NONE,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_picked_video_source(url: AstralText) -> AstralOwned {
    match app::picked_media::video_file(&url.text()) {
        app::picked_media::VideoFile::Source(source) => AstralOwned::text(source),
        _ => AstralOwned::NONE,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_remember_picked_video_file(url: AstralText, file: AstralText) {
    app::picked_media::remember_video_file(&url.text(), &file.text());
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_ios_byte_range(
    header: AstralText,
    length: u64,
    start: *mut u64,
    count: *mut u64,
) -> bool {
    let Some((first, size)) = app::picked_media::byte_range(&header.text(), length) else {
        return false;
    };
    if start.is_null() || count.is_null() {
        return false;
    }
    unsafe {
        start.write(first);
        count.write(size);
    }
    true
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_web_auth_note_initial_cookies(
    request: u64,
    cookies: AstralText,
    url: AstralText,
) {
    app::web_auth::note_initial_cookies(
        request,
        &app::web_auth::cookie_header(&cookies.text(), &url.text()),
    );
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_web_auth_is_callback(request: u64, url: AstralText) -> bool {
    app::web_auth::is_callback(request, &url.text())
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_web_auth_new_cookies(
    request: u64,
    cookies: AstralText,
    url: AstralText,
) -> AstralOwned {
    AstralOwned::text(app::web_auth::new_cookies(
        request,
        &app::web_auth::cookie_header(&cookies.text(), &url.text()),
    ))
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_browser_login_url() -> AstralOwned {
    AstralOwned::text(app::web_auth::browser_login_url())
}

#[repr(C)]
pub struct AstralReadiness {
    pub action: i32,
    pub report: bool,
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_write_status(stage: AstralText, facts: AstralText) {
    app::launch::write_status(&stage.text(), &facts.text());
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_last_stage() -> AstralOwned {
    AstralOwned::text(app::launch::last_stage())
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_claim_start() -> bool {
    app::launch::claim_start()
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_launch_started() -> bool {
    app::launch::started()
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_launch_foregrounded() -> bool {
    app::launch::foregrounded()
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_check_readiness(compatibility: AstralText) -> AstralReadiness {
    let readiness = app::launch::check_readiness(&compatibility.text());
    AstralReadiness {
        action: readiness.action,
        report: readiness.report,
    }
}
