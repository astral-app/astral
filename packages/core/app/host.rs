use std::sync::{Arc, RwLock};

use serde_json::Value;

pub enum Request<'a> {
    ShowAlert {
        title: &'a str,
        message: &'a str,
    },
    ShowToast {
        title: &'a str,
        message: &'a str,
    },
    TriggerHaptic {
        strength: &'a str,
    },
    WriteClipboard {
        value: &'a str,
    },
    ReadClipboard,
    OpenUrl {
        url: &'a str,
    },
    Notify {
        delay_ms: i64,
        title: &'a str,
        payload: &'a str,
    },
    Speak {
        rate_permille: i64,
        text: &'a str,
        language: &'a str,
    },
    PlaySound {
        volume_permille: i64,
        path: &'a str,
    },
    StopAudio,
    NetworkRequest {
        request: u64,
        options: &'a str,
    },
    CancelNetworkRequest {
        request: u64,
    },
    WebAuth {
        request: u64,
        url: &'a str,
        callback_url: &'a str,
    },
    CancelWebAuth {
        request: u64,
    },
    Confirm {
        request: u64,
        spec: &'a str,
    },
    OpenSheet {
        sheet: u64,
        spec: &'a str,
    },
    UpdateSheet {
        sheet: u64,
        spec: &'a str,
    },
    CloseSheet {
        sheet: u64,
    },
    PickFile {
        request: u64,
        extensions: &'a str,
    },
    CancelFilePick {
        request: u64,
    },
    PickedMedia {
        index: u64,
    },
    PickedVideo {
        index: u64,
    },
    PressNode {
        identifier: &'a str,
        prefix: bool,
    },
    InsertComposerText {
        text: &'a str,
        replace: bool,
    },
    RenderImage {
        spec: &'a str,
    },
    DispatchGateway {
        message: &'a str,
    },
    RefreshEmoji,
    OpenReport {
        text: &'a str,
        url: &'a str,
    },
    ExportBackup,
    ImportBackup,
    RestartGatewaySession,
    EditText {
        request: u64,
        title: &'a str,
        value: &'a str,
        maximum: i32,
    },
    EnvironmentFacts,
    Fetch {
        url: &'a str,
        token: &'a str,
    },
    FetchWith {
        url: &'a str,
        headers: &'a str,
    },
    KeepAlive {
        on: bool,
    },
    OfferPick {
        name: &'a str,
        data: &'a [u8],
    },
    DismissSheets,
    RefreshDecorations,
    RefreshSettings,
    InjectGateway {
        message: &'a str,
    },
}

pub enum Reply {
    Done,
    Flag(bool),
    Text(Option<String>),
    Bytes(Option<Vec<u8>>),
    Unsupported,
}

impl Reply {
    pub fn flag(&self) -> bool {
        matches!(self, Reply::Done | Reply::Flag(true))
    }

    pub fn into_text(self) -> Option<String> {
        match self {
            Reply::Text(text) => text,
            _ => None,
        }
    }

    pub fn into_bytes(self) -> Option<Vec<u8>> {
        match self {
            Reply::Bytes(bytes) => bytes,
            _ => None,
        }
    }

    pub fn supported(&self) -> bool {
        !matches!(self, Reply::Unsupported)
    }
}

pub trait Bridge: Send + Sync {
    fn deliver(&self, bytes: &[u8]);
    fn request(&self, request: Request) -> Reply;
    fn log(&self, line: &str);
}

static BRIDGE: RwLock<Option<Arc<dyn Bridge>>> = RwLock::new(None);

pub fn set_bridge(bridge: Arc<dyn Bridge>) {
    *BRIDGE.write().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(bridge);
}

fn current() -> Option<Arc<dyn Bridge>> {
    BRIDGE.read().unwrap_or_else(|poisoned| poisoned.into_inner()).clone()
}

pub fn request(request: Request) -> Reply {
    current().map_or(Reply::Unsupported, |bridge| bridge.request(request))
}

pub fn deliver(bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    if let Some(bridge) = current() {
        bridge.deliver(bytes);
    }
}

pub fn log(line: &str) {
    if let Some(bridge) = current() {
        bridge.log(line);
    }
}

pub fn json_text(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_default()
}
