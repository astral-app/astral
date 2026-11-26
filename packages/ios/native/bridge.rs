use std::sync::Arc;

use astral_core::app::host::{Bridge, Reply, Request};

use crate::text::AstralText;

#[repr(u32)]
#[derive(Clone, Copy)]
pub enum AstralRequestKind {
    ShowAlert,
    ShowToast,
    TriggerHaptic,
    WriteClipboard,
    ReadClipboard,
    OpenUrl,
    Notify,
    Speak,
    PlaySound,
    StopAudio,
    NetworkRequest,
    CancelNetworkRequest,
    WebAuth,
    CancelWebAuth,
    Confirm,
    OpenSheet,
    UpdateSheet,
    CloseSheet,
    PickFile,
    CancelFilePick,
    PickedMedia,
    PickedVideo,
    PressNode,
    InsertComposerText,
    RenderImage,
    DispatchGateway,
    RefreshEmoji,
    OpenReport,
    ExportBackup,
    ImportBackup,
    RestartGatewaySession,
    EditText,
    EnvironmentFacts,
    Fetch,
    DiscordRequest,
    KeepAlive,
    OfferPick,
    DismissSheets,
    RefreshDecorations,
    RefreshSettings,
    InjectGateway,
}

#[repr(C)]
pub struct AstralRequest {
    pub kind: AstralRequestKind,
    pub identifier: u64,
    pub number: i64,
    pub flag: bool,
    pub first: AstralText,
    pub second: AstralText,
    pub third: AstralText,
}

#[repr(u32)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AstralReplyKind {
    Done,
    Flag,
    Text,
    Bytes,
    Unsupported,
}

#[repr(C)]
pub struct AstralReply {
    pub kind: AstralReplyKind,
    pub flag: bool,
    pub bytes: *mut u8,
    pub length: usize,
}

pub type AstralDeliver = unsafe extern "C" fn(bytes: *const u8, length: usize);
pub type AstralHandle = unsafe extern "C" fn(request: *const AstralRequest) -> AstralReply;
pub type AstralLog = unsafe extern "C" fn(line: AstralText);
pub type AstralWireLock = unsafe extern "C" fn();

#[repr(C)]
#[derive(Clone, Copy)]
pub struct AstralBridge {
    pub deliver: AstralDeliver,
    pub handle: AstralHandle,
    pub log: AstralLog,
    pub lock_wire: AstralWireLock,
    pub unlock_wire: AstralWireLock,
}

struct Outgoing<'a> {
    kind: AstralRequestKind,
    identifier: u64,
    number: i64,
    flag: bool,
    texts: [&'a [u8]; 3],
}

impl<'a> Outgoing<'a> {
    fn new(kind: AstralRequestKind) -> Outgoing<'a> {
        Outgoing {
            kind,
            identifier: 0,
            number: 0,
            flag: false,
            texts: [&[], &[], &[]],
        }
    }

    fn identifier(mut self, identifier: u64) -> Outgoing<'a> {
        self.identifier = identifier;
        self
    }

    fn number(mut self, number: i64) -> Outgoing<'a> {
        self.number = number;
        self
    }

    fn flag(mut self, flag: bool) -> Outgoing<'a> {
        self.flag = flag;
        self
    }

    fn texts(mut self, texts: &[&'a str]) -> Outgoing<'a> {
        for (slot, text) in self.texts.iter_mut().zip(texts) {
            *slot = text.as_bytes();
        }
        self
    }

    fn data(mut self, data: &'a [u8]) -> Outgoing<'a> {
        self.texts[1] = data;
        self
    }

    fn request(&self) -> AstralRequest {
        let [first, second, third] = self.texts;
        AstralRequest {
            kind: self.kind,
            identifier: self.identifier,
            number: self.number,
            flag: self.flag,
            first: AstralText::of(first),
            second: AstralText::of(second),
            third: AstralText::of(third),
        }
    }
}

fn outgoing<'a>(request: &Request<'a>) -> Outgoing<'a> {
    use AstralRequestKind as Kind;
    match *request {
        Request::ShowAlert { title, message } => {
            Outgoing::new(Kind::ShowAlert).texts(&[title, message])
        }
        Request::ShowToast { title, message } => {
            Outgoing::new(Kind::ShowToast).texts(&[title, message])
        }
        Request::TriggerHaptic { strength } => {
            Outgoing::new(Kind::TriggerHaptic).texts(&[strength])
        }
        Request::WriteClipboard { value } => Outgoing::new(Kind::WriteClipboard).texts(&[value]),
        Request::ReadClipboard => Outgoing::new(Kind::ReadClipboard),
        Request::OpenUrl { url } => Outgoing::new(Kind::OpenUrl).texts(&[url]),
        Request::Notify {
            delay_ms,
            title,
            payload,
        } => Outgoing::new(Kind::Notify)
            .number(delay_ms)
            .texts(&[title, payload]),
        Request::Speak {
            rate_permille,
            text,
            language,
        } => Outgoing::new(Kind::Speak)
            .number(rate_permille)
            .texts(&[text, language]),
        Request::PlaySound {
            volume_permille,
            path,
        } => Outgoing::new(Kind::PlaySound)
            .number(volume_permille)
            .texts(&[path]),
        Request::StopAudio => Outgoing::new(Kind::StopAudio),
        Request::NetworkRequest { request, options } => Outgoing::new(Kind::NetworkRequest)
            .identifier(request)
            .texts(&[options]),
        Request::CancelNetworkRequest { request } => {
            Outgoing::new(Kind::CancelNetworkRequest).identifier(request)
        }
        Request::WebAuth {
            request,
            url,
            callback_url,
        } => Outgoing::new(Kind::WebAuth)
            .identifier(request)
            .texts(&[url, callback_url]),
        Request::CancelWebAuth { request } => {
            Outgoing::new(Kind::CancelWebAuth).identifier(request)
        }
        Request::Confirm { request, spec } => Outgoing::new(Kind::Confirm)
            .identifier(request)
            .texts(&[spec]),
        Request::OpenSheet { sheet, spec } => Outgoing::new(Kind::OpenSheet)
            .identifier(sheet)
            .texts(&[spec]),
        Request::UpdateSheet { sheet, spec } => Outgoing::new(Kind::UpdateSheet)
            .identifier(sheet)
            .texts(&[spec]),
        Request::CloseSheet { sheet } => Outgoing::new(Kind::CloseSheet).identifier(sheet),
        Request::PickFile {
            request,
            extensions,
        } => Outgoing::new(Kind::PickFile)
            .identifier(request)
            .texts(&[extensions]),
        Request::CancelFilePick { request } => {
            Outgoing::new(Kind::CancelFilePick).identifier(request)
        }
        Request::PickedMedia { index } => Outgoing::new(Kind::PickedMedia).identifier(index),
        Request::PickedVideo { index } => Outgoing::new(Kind::PickedVideo).identifier(index),
        Request::PressNode { identifier, prefix } => Outgoing::new(Kind::PressNode)
            .flag(prefix)
            .texts(&[identifier]),
        Request::InsertComposerText { text, replace } => Outgoing::new(Kind::InsertComposerText)
            .flag(replace)
            .texts(&[text]),
        Request::RenderImage { spec } => Outgoing::new(Kind::RenderImage).texts(&[spec]),
        Request::DispatchGateway { message } => {
            Outgoing::new(Kind::DispatchGateway).texts(&[message])
        }
        Request::RefreshEmoji => Outgoing::new(Kind::RefreshEmoji),
        Request::OpenReport { text, url } => Outgoing::new(Kind::OpenReport).texts(&[text, url]),
        Request::ExportBackup => Outgoing::new(Kind::ExportBackup),
        Request::ImportBackup => Outgoing::new(Kind::ImportBackup),
        Request::RestartGatewaySession => Outgoing::new(Kind::RestartGatewaySession),
        Request::EditText {
            request,
            title,
            value,
            maximum,
        } => Outgoing::new(Kind::EditText)
            .identifier(request)
            .number(i64::from(maximum))
            .texts(&[title, value]),
        Request::EnvironmentFacts => Outgoing::new(Kind::EnvironmentFacts),
        Request::Fetch { url, token } => Outgoing::new(Kind::Fetch).texts(&[url, token]),
        Request::DiscordRequest {
            request,
            method,
            path,
            body,
        } => Outgoing::new(Kind::DiscordRequest)
            .identifier(request)
            .texts(&[method, path, body]),
        Request::KeepAlive { on } => Outgoing::new(Kind::KeepAlive).flag(on),
        Request::OfferPick { name, data } => {
            Outgoing::new(Kind::OfferPick).texts(&[name]).data(data)
        }
        Request::DismissSheets => Outgoing::new(Kind::DismissSheets),
        Request::RefreshDecorations => Outgoing::new(Kind::RefreshDecorations),
        Request::RefreshSettings => Outgoing::new(Kind::RefreshSettings),
        Request::InjectGateway { message } => Outgoing::new(Kind::InjectGateway).texts(&[message]),
    }
}

fn adopt_bytes(reply: &AstralReply) -> Option<Vec<u8>> {
    if reply.bytes.is_null() {
        return None;
    }
    let copied = unsafe { core::slice::from_raw_parts(reply.bytes, reply.length) }.to_vec();
    unsafe { libc::free(reply.bytes.cast()) };
    Some(copied)
}

fn adopt(reply: &AstralReply) -> Reply {
    let bytes = adopt_bytes(reply);
    match reply.kind {
        AstralReplyKind::Done => Reply::Done,
        AstralReplyKind::Flag => Reply::Flag(reply.flag),
        AstralReplyKind::Text => {
            Reply::Text(bytes.map(|bytes| String::from_utf8_lossy(&bytes).into_owned()))
        }
        AstralReplyKind::Bytes => Reply::Bytes(bytes),
        AstralReplyKind::Unsupported => Reply::Unsupported,
    }
}

pub struct CallbackBridge(pub AstralBridge);

impl Bridge for CallbackBridge {
    fn deliver(&self, bytes: &[u8]) {
        unsafe { (self.0.deliver)(bytes.as_ptr(), bytes.len()) };
    }

    fn request(&self, request: Request) -> Reply {
        let outgoing = outgoing(&request).request();
        adopt(&unsafe { (self.0.handle)(&raw const outgoing) })
    }

    fn log(&self, line: &str) {
        unsafe { (self.0.log)(AstralText::of(line.as_bytes())) };
    }
}

pub fn attach(bridge: AstralBridge) {
    astral_core::app::state::share_wire_lock(bridge.lock_wire, bridge.unlock_wire);
    astral_core::app::set_bridge(Arc::new(CallbackBridge(bridge)));
}
