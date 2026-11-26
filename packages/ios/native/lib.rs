#![allow(clippy::missing_safety_doc)]

mod bridge;
mod network;
mod shapes;
mod text;

use astral_core::app::{self, Launch, state::Platform};
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
pub extern "C" fn astral_ios_press_inserted_node(identifier: AstralText) -> bool {
    app::press_inserted_identifier(&identifier.text())
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_press_message_action(identifier: AstralText) -> bool {
    app::press_message_identifier(&identifier.text())
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
pub extern "C" fn astral_ios_refresh_decorations() {
    app::refresh_decorations();
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_recovering() -> bool {
    app::state::app().is_some_and(|current| current.recovering)
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
pub extern "C" fn astral_ios_navigator_handles(identifier: AstralText) -> bool {
    astral_core::settings::navigator::handles(&identifier.text())
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_navigator_tap(identifier: AstralText) {
    astral_core::settings::navigator::tap(&identifier.text());
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_navigator_slide(identifier: AstralText, fraction: f64) {
    astral_core::settings::navigator::slide(&identifier.text(), fraction);
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_navigator_back() -> bool {
    astral_core::settings::navigator::back()
}
