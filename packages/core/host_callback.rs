use core::ffi::{c_char, c_int, c_void};
use std::sync::Arc;

use crate::bridge::bytes;

pub type Invoke = unsafe extern "C" fn(context: *mut c_void, value: *const c_char, length: usize, present: c_int);
pub type Release = unsafe extern "C" fn(context: *mut c_void);

#[repr(C)]
pub struct AstralCallback {
    pub invoke: Option<Invoke>,
    pub release: Option<Release>,
    pub context: *mut c_void,
}

pub type Reply = Arc<dyn Fn(Option<String>) + Send + Sync>;
pub type Changed = Arc<dyn Fn() + Send + Sync>;

impl AstralCallback {
    pub fn none() -> AstralCallback {
        AstralCallback {
            invoke: None,
            release: None,
            context: core::ptr::null_mut(),
        }
    }

    pub fn from_reply(reply: Reply) -> AstralCallback {
        AstralCallback {
            invoke: Some(invoke_reply),
            release: Some(release_reply),
            context: Box::into_raw(Box::new(reply)).cast(),
        }
    }

    pub fn from_changed(changed: Changed) -> AstralCallback {
        AstralCallback::from_reply(Arc::new(move |_| changed()))
    }
}

unsafe extern "C" fn invoke_reply(context: *mut c_void, value: *const c_char, length: usize, present: c_int) {
    let Some(reply) = (unsafe { context.cast::<Reply>().as_ref() }) else {
        return;
    };
    let value = (present != 0).then(|| String::from_utf8_lossy(bytes(value, length)).into_owned());
    reply(value);
}

unsafe extern "C" fn release_reply(context: *mut c_void) {
    if !context.is_null() {
        drop(unsafe { Box::from_raw(context.cast::<Reply>()) });
    }
}

struct Foreign(AstralCallback);

unsafe impl Send for Foreign {}
unsafe impl Sync for Foreign {}

impl Foreign {
    fn call(&self, value: Option<&str>) {
        let Some(invoke) = self.0.invoke else { return };
        let (data, length) = value.map_or((core::ptr::null(), 0), |value| (value.as_ptr().cast(), value.len()));
        unsafe { invoke(self.0.context, data, length, c_int::from(value.is_some())) };
    }
}

impl Drop for Foreign {
    fn drop(&mut self) {
        if let Some(release) = self.0.release {
            unsafe { release(self.0.context) };
        }
    }
}

pub fn adopt_reply(callback: AstralCallback) -> Option<Reply> {
    let foreign = Foreign(callback);
    foreign.0.invoke?;
    let foreign = Arc::new(foreign);
    Some(Arc::new(move |value: Option<String>| foreign.call(value.as_deref())))
}

pub fn adopt_changed(callback: AstralCallback) -> Changed {
    match adopt_reply(callback) {
        Some(reply) => Arc::new(move || reply(None)),
        None => Arc::new(|| {}),
    }
}
