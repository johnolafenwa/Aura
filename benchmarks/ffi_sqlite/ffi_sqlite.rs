// The Rust side of the SQLite benchmark: the same C calls through raw
// `extern "C"` declarations, with no binding crate.
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::time::Instant;

#[link(name = "sqlite3")]
extern "C" {
    fn sqlite3_open(path: *const c_char, out: *mut *mut c_void) -> c_int;
    fn sqlite3_close_v2(db: *mut c_void) -> c_int;
    fn sqlite3_exec(
        db: *mut c_void,
        sql: *const c_char,
        row: Option<extern "C" fn(*mut c_void, c_int, *mut *mut c_char, *mut *mut c_char) -> c_int>,
        data: *mut c_void,
        error: *mut *mut c_char,
    ) -> c_int;
    fn sqlite3_prepare_v2(
        db: *mut c_void,
        sql: *const c_char,
        length: c_int,
        out: *mut *mut c_void,
        tail: *mut *const c_char,
    ) -> c_int;
    fn sqlite3_bind_int64(statement: *mut c_void, index: c_int, value: i64) -> c_int;
    fn sqlite3_bind_text(
        statement: *mut c_void,
        index: c_int,
        text: *const c_char,
        length: c_int,
        lifetime: isize,
    ) -> c_int;
    fn sqlite3_step(statement: *mut c_void) -> c_int;
    fn sqlite3_reset(statement: *mut c_void) -> c_int;
    fn sqlite3_column_int64(statement: *mut c_void, column: c_int) -> i64;
    fn sqlite3_column_text(statement: *mut c_void, column: c_int) -> *const c_char;
    fn sqlite3_finalize(statement: *mut c_void) -> c_int;
}

extern "C" fn no_rows(_: *mut c_void, _: c_int, _: *mut *mut c_char, _: *mut *mut c_char) -> c_int {
    0
}

unsafe fn exec(db: *mut c_void, sql: &str) {
    let sql = CString::new(sql).unwrap();
    sqlite3_exec(db, sql.as_ptr(), Some(no_rows), std::ptr::null_mut(), std::ptr::null_mut());
}

unsafe fn prepare(db: *mut c_void, sql: &str) -> *mut c_void {
    let sql = CString::new(sql).unwrap();
    let mut statement = std::ptr::null_mut();
    sqlite3_prepare_v2(db, sql.as_ptr(), -1, &mut statement, std::ptr::null_mut());
    statement
}

/// The text of a column as an owned string, as the Aura side copies it.
unsafe fn column_text(statement: *mut c_void, column: c_int) -> String {
    CStr::from_ptr(sqlite3_column_text(statement, column))
        .to_str()
        .unwrap()
        .to_owned()
}

fn main() {
    let rows: i64 = 500_000;
    unsafe {
        let mut db = std::ptr::null_mut();
        let path = CString::new(":memory:").unwrap();
        sqlite3_open(path.as_ptr(), &mut db);
        exec(db, "create table notes (id integer primary key, body text not null)");

        let started = Instant::now();
        let insert = prepare(db, "insert into notes (id, body) values (?, ?)");
        exec(db, "begin");
        for i in 0..rows {
            sqlite3_bind_int64(insert, 1, i);
            let body = CString::new(format!("note number {i}")).unwrap();
            sqlite3_bind_text(insert, 2, body.as_ptr(), -1, -1);
            sqlite3_step(insert);
            sqlite3_reset(insert);
        }
        exec(db, "commit");
        sqlite3_finalize(insert);
        println!("insert {}", started.elapsed().as_millis());

        let started = Instant::now();
        let query = prepare(db, "select id, body from notes");
        let mut read: i64 = 0;
        while sqlite3_step(query) == 100 {
            read += sqlite3_column_int64(query, 0);
            read += column_text(query, 1).len() as i64;
        }
        sqlite3_finalize(query);
        println!("read {}", started.elapsed().as_millis());

        let started = Instant::now();
        let query = prepare(db, "select body from notes where id = ?");
        let mut found: i64 = 0;
        for i in 0..rows {
            sqlite3_bind_int64(query, 1, (i * 7919) % rows);
            if sqlite3_step(query) == 100 {
                found += column_text(query, 0).len() as i64;
            }
            sqlite3_reset(query);
        }
        sqlite3_finalize(query);
        println!("lookup {}", started.elapsed().as_millis());
        println!("checksum {read} {found}");
        sqlite3_close_v2(db);
    }
}
