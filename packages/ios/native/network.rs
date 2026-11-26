use astral_core::app::{self, network};

use crate::text::{AstralOwned, AstralText};

#[repr(C)]
pub struct AstralRequestProbe {
    pub method: AstralText,
    pub url: AstralText,
    pub host: AstralText,
    pub path: AstralText,
    pub headers: AstralText,
    pub json_body: bool,
}

#[repr(C)]
#[derive(Default)]
pub struct AstralRequestPlan {
    pub block: bool,
    pub mark_staff: bool,
    pub transform_message: bool,
    pub intercept: bool,
    pub rewrite_image: bool,
    pub handles: bool,
}

fn with_probe<T>(probe: &AstralRequestProbe, work: impl FnOnce(&network::Probe) -> T) -> T {
    let (method, url, host, path, headers) = (
        probe.method.text(),
        probe.url.text(),
        probe.host.text(),
        probe.path.text(),
        probe.headers.text(),
    );
    work(&network::Probe {
        method: &method,
        url: &url,
        host: &host,
        path: &path,
        headers: &headers,
        json_body: probe.json_body,
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_ios_note_request(probe: *const AstralRequestProbe) {
    if let Some(probe) = unsafe { probe.as_ref() } {
        with_probe(probe, network::note);
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_ios_plan_request(
    probe: *const AstralRequestProbe,
) -> AstralRequestPlan {
    let Some(probe) = (unsafe { probe.as_ref() }) else {
        return AstralRequestPlan::default();
    };
    let plan = with_probe(probe, network::plan);
    AstralRequestPlan {
        block: plan.block,
        mark_staff: plan.mark_staff,
        transform_message: plan.transform_message,
        intercept: plan.intercept,
        rewrite_image: plan.rewrite_image,
        handles: plan.handles(),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_rewrite_image(
    method: AstralText,
    host: AstralText,
    url: AstralText,
) -> AstralOwned {
    AstralOwned::optional(network::rewrite_image(
        &method.text(),
        &host.text(),
        &url.text(),
    ))
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_transform_outgoing(
    method: AstralText,
    host: AstralText,
    path: AstralText,
    body: AstralText,
) -> AstralOwned {
    network::transform_outgoing(&method.text(), &host.text(), &path.text(), body.bytes())
        .map_or(AstralOwned::NONE, AstralOwned::new)
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_intercept_request(request: AstralText) -> AstralOwned {
    let decision = network::intercept_request(&request.text());
    if decision.is_null() {
        return AstralOwned::NONE;
    }
    AstralOwned::text(decision.to_string())
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_intercept_response(
    request: AstralText,
    response: AstralText,
) -> AstralOwned {
    AstralOwned::optional(
        network::intercept_response(&request.text(), &response.text())
            .map(|value| value.to_string()),
    )
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_marks_staff(
    method: AstralText,
    host: AstralText,
    path: AstralText,
) -> bool {
    network::marks_staff(&method.text(), &host.text(), &path.text())
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_mark_staff_user(body: AstralText) -> AstralOwned {
    network::mark_staff_user(body.bytes()).map_or(AstralOwned::NONE, AstralOwned::new)
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_mark_staff_ready(message: AstralText) -> AstralOwned {
    AstralOwned::optional(network::mark_staff_ready(&message.text()))
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_note_compressed_frame() {
    network::note_compressed_frame();
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_note_blocked_request() {
    app::note_blocked_request();
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_process_gateway(message: AstralText) -> AstralOwned {
    AstralOwned::optional(app::process_gateway_message(&message.text()))
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_wants_uncompressed_gateway() -> bool {
    network::wants_uncompressed_gateway()
}

#[repr(u32)]
#[derive(Clone, Copy)]
pub enum AstralMediaKind {
    Image,
    Video,
}

impl AstralMediaKind {
    const fn kind(self) -> astral_core::lua_enums::MediaKind {
        match self {
            AstralMediaKind::Image => astral_core::lua_enums::MediaKind::Image,
            AstralMediaKind::Video => astral_core::lua_enums::MediaKind::Video,
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_has_media_save_handlers() -> bool {
    network::has_media_save_handlers()
}

#[unsafe(no_mangle)]
pub extern "C" fn astral_ios_replace_saved_media(
    url: AstralText,
    kind: AstralMediaKind,
) -> AstralOwned {
    AstralOwned::optional(network::replace_saved_media(&url.text(), kind.kind()))
}
