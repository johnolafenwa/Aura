"""The Python side of the SQLite benchmark, twice: the standard library's
`sqlite3` module, which is a C extension, and the same C calls through
ctypes, which is how Python binds a library that has no extension."""
import ctypes
import ctypes.util
import sqlite3
import time

ROWS = 500_000


def report(name, started):
    print(f'{name} {(time.perf_counter_ns() - started) // 1_000_000}')


def stdlib():
    db = sqlite3.connect(':memory:')
    db.execute('create table notes (id integer primary key, body text not null)')
    started = time.perf_counter_ns()
    with db:
        insert = 'insert into notes (id, body) values (?, ?)'
        for i in range(ROWS):
            db.execute(insert, (i, f'note number {i}'))
    report('stdlib_insert', started)

    started = time.perf_counter_ns()
    read = 0
    for row_id, body in db.execute('select id, body from notes'):
        read += row_id + len(body)
    report('stdlib_read', started)

    started = time.perf_counter_ns()
    found = 0
    query = 'select body from notes where id = ?'
    for i in range(ROWS):
        row = db.execute(query, ((i * 7919) % ROWS,)).fetchone()
        if row is not None:
            found += len(row[0])
    report('stdlib_lookup', started)
    print(f'checksum {read} {found}')


def with_ctypes():
    lib = ctypes.CDLL(ctypes.util.find_library('sqlite3'))
    pointer, text, i32, i64 = ctypes.c_void_p, ctypes.c_char_p, ctypes.c_int, ctypes.c_int64
    row = ctypes.CFUNCTYPE(i32, pointer, i32, pointer, pointer)
    lib.sqlite3_open.argtypes = [text, ctypes.POINTER(pointer)]
    lib.sqlite3_exec.argtypes = [pointer, text, row, pointer, pointer]
    lib.sqlite3_prepare_v2.argtypes = [pointer, text, i32, ctypes.POINTER(pointer), pointer]
    lib.sqlite3_bind_int64.argtypes = [pointer, i32, i64]
    lib.sqlite3_bind_text.argtypes = [pointer, i32, text, i32, ctypes.c_ssize_t]
    lib.sqlite3_step.argtypes = [pointer]
    lib.sqlite3_reset.argtypes = [pointer]
    lib.sqlite3_column_int64.argtypes, lib.sqlite3_column_int64.restype = [pointer, i32], i64
    lib.sqlite3_column_text.argtypes, lib.sqlite3_column_text.restype = [pointer, i32], text
    lib.sqlite3_finalize.argtypes = [pointer]
    lib.sqlite3_close_v2.argtypes = [pointer]
    no_rows = row(lambda data, columns, values, names: 0)

    def prepare(sql):
        statement = pointer()
        lib.sqlite3_prepare_v2(db, sql, -1, ctypes.byref(statement), None)
        return statement

    db = pointer()
    lib.sqlite3_open(b':memory:', ctypes.byref(db))
    lib.sqlite3_exec(
        db, b'create table notes (id integer primary key, body text not null)', no_rows, None, None
    )

    started = time.perf_counter_ns()
    insert = prepare(b'insert into notes (id, body) values (?, ?)')
    lib.sqlite3_exec(db, b'begin', no_rows, None, None)
    for i in range(ROWS):
        lib.sqlite3_bind_int64(insert, 1, i)
        lib.sqlite3_bind_text(insert, 2, f'note number {i}'.encode(), -1, -1)
        lib.sqlite3_step(insert)
        lib.sqlite3_reset(insert)
    lib.sqlite3_exec(db, b'commit', no_rows, None, None)
    lib.sqlite3_finalize(insert)
    report('ctypes_insert', started)

    started = time.perf_counter_ns()
    query = prepare(b'select id, body from notes')
    read = 0
    while lib.sqlite3_step(query) == 100:
        read += lib.sqlite3_column_int64(query, 0)
        read += len(lib.sqlite3_column_text(query, 1).decode())
    lib.sqlite3_finalize(query)
    report('ctypes_read', started)

    started = time.perf_counter_ns()
    query = prepare(b'select body from notes where id = ?')
    found = 0
    for i in range(ROWS):
        lib.sqlite3_bind_int64(query, 1, (i * 7919) % ROWS)
        if lib.sqlite3_step(query) == 100:
            found += len(lib.sqlite3_column_text(query, 0).decode())
        lib.sqlite3_reset(query)
    lib.sqlite3_finalize(query)
    report('ctypes_lookup', started)
    print(f'checksum {read} {found}')
    lib.sqlite3_close_v2(db)


stdlib()
with_ctypes()
