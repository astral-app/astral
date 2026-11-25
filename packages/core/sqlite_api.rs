use core::ffi::{c_char, c_int, c_void};
use std::ffi::CString;

pub enum Sqlite3 {}
pub enum Statement3 {}

pub const OK: c_int = 0;
pub const ROW: c_int = 100;
pub const DONE: c_int = 101;
pub const ERROR: c_int = 1;
pub const OPEN_READONLY: c_int = 0x0000_0001;
pub const OPEN_READWRITE: c_int = 0x0000_0002;
pub const OPEN_CREATE: c_int = 0x0000_0004;
pub const OPEN_URI: c_int = 0x0000_0040;
pub const OPEN_NOMUTEX: c_int = 0x0000_8000;
pub const OPEN_FULLMUTEX: c_int = 0x0001_0000;

type Destructor = isize;
const TRANSIENT: Destructor = -1;

unsafe extern "C" {
    fn sqlite3_open_v2(path: *const c_char, database: *mut *mut Sqlite3, flags: c_int, vfs: *const c_char) -> c_int;
    fn sqlite3_close(database: *mut Sqlite3) -> c_int;
    fn sqlite3_exec(
        database: *mut Sqlite3,
        sql: *const c_char,
        callback: *const c_void,
        argument: *mut c_void,
        error: *mut *mut c_char,
    ) -> c_int;
    fn sqlite3_busy_timeout(database: *mut Sqlite3, milliseconds: c_int) -> c_int;
    fn sqlite3_prepare_v2(
        database: *mut Sqlite3,
        sql: *const c_char,
        length: c_int,
        statement: *mut *mut Statement3,
        tail: *mut *const c_char,
    ) -> c_int;
    fn sqlite3_bind_text(statement: *mut Statement3, position: c_int, text: *const c_char, length: c_int, destructor: Destructor) -> c_int;
    fn sqlite3_step(statement: *mut Statement3) -> c_int;
    fn sqlite3_finalize(statement: *mut Statement3) -> c_int;
    fn sqlite3_column_blob(statement: *mut Statement3, column: c_int) -> *const c_void;
    fn sqlite3_column_text(statement: *mut Statement3, column: c_int) -> *const u8;
    fn sqlite3_column_bytes(statement: *mut Statement3, column: c_int) -> c_int;
    fn sqlite3_column_int64(statement: *mut Statement3, column: c_int) -> i64;
}

pub struct Database(*mut Sqlite3);

unsafe impl Send for Database {}
unsafe impl Sync for Database {}

impl Database {
    pub fn open(path: &str, flags: c_int) -> Option<Database> {
        let path = CString::new(path).ok()?;
        let mut handle = core::ptr::null_mut();
        let status = unsafe { sqlite3_open_v2(path.as_ptr(), &mut handle, flags, core::ptr::null()) };
        if status != OK {
            unsafe { sqlite3_close(handle) };
            return None;
        }
        Some(Database(handle))
    }

    pub fn execute(&self, sql: &str) -> bool {
        let Ok(sql) = CString::new(sql) else { return false };
        unsafe {
            sqlite3_exec(
                self.0,
                sql.as_ptr(),
                core::ptr::null(),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
            ) == OK
        }
    }

    pub fn busy_timeout(&self, milliseconds: c_int) {
        unsafe { sqlite3_busy_timeout(self.0, milliseconds) };
    }

    pub fn prepare(&self, sql: &str, values: &[&[u8]]) -> Statement {
        let mut statement = core::ptr::null_mut();
        let Ok(text) = CString::new(sql) else {
            return Statement(core::ptr::null_mut());
        };
        if unsafe { sqlite3_prepare_v2(self.0, text.as_ptr(), -1, &mut statement, core::ptr::null_mut()) } != OK {
            return Statement(core::ptr::null_mut());
        }
        for (index, value) in values.iter().enumerate() {
            unsafe {
                sqlite3_bind_text(
                    statement,
                    index as c_int + 1,
                    value.as_ptr().cast(),
                    value.len() as c_int,
                    TRANSIENT,
                )
            };
        }
        Statement(statement)
    }
}

impl Drop for Database {
    fn drop(&mut self) {
        unsafe { sqlite3_close(self.0) };
    }
}

pub struct Statement(*mut Statement3);

impl Statement {
    pub fn step(&mut self) -> c_int {
        if self.0.is_null() { ERROR } else { unsafe { sqlite3_step(self.0) } }
    }

    pub fn blob(&self, column: c_int) -> Vec<u8> {
        let data = unsafe { sqlite3_column_blob(self.0, column) };
        let length = unsafe { sqlite3_column_bytes(self.0, column) };
        if data.is_null() || length <= 0 {
            return Vec::new();
        }
        unsafe { core::slice::from_raw_parts(data.cast::<u8>(), length as usize) }.to_vec()
    }

    pub fn text(&self, column: c_int) -> Vec<u8> {
        let data = unsafe { sqlite3_column_text(self.0, column) };
        let length = unsafe { sqlite3_column_bytes(self.0, column) };
        if data.is_null() || length <= 0 {
            return Vec::new();
        }
        unsafe { core::slice::from_raw_parts(data, length as usize) }.to_vec()
    }

    pub fn integer(&self, column: c_int) -> i64 {
        unsafe { sqlite3_column_int64(self.0, column) }
    }
}

impl Drop for Statement {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { sqlite3_finalize(self.0) };
        }
    }
}
