//! paperpaw-sqlite against SQLite itself (through `rusqlite`, built from
//! SQLite's sources): files the writer makes pass SQLite's integrity
//! check and hold the rows given, indexes included; files SQLite makes
//! (other page sizes, WAL, auto-vacuum, free pages, KOReader's schema)
//! are read; records sort as SQLite sorts them.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use std::cmp::Ordering;

use common::{COLUMNS, Memory, TempFile, index_entry, read_rows, table_entry, text};
use paperpaw_sqlite::{Database, Error, Value, Writer, compare_records, encode_record};
use proptest::prelude::*;
use rusqlite::Connection;
use rusqlite::types::Value as Sql;

fn to_sql(value: &Value) -> Sql {
    match value {
        Value::Null => Sql::Null,
        Value::Int(v) => Sql::Integer(*v),
        Value::Real(v) => Sql::Real(*v),
        Value::Text(t) => Sql::Text(t.clone()),
        Value::Blob(b) => Sql::Blob(b.clone()),
    }
}

fn from_sql(value: Sql) -> Value {
    match value {
        Sql::Null => Value::Null,
        Sql::Integer(v) => Value::Int(v),
        Sql::Real(v) => Value::Real(v),
        Sql::Text(t) => Value::Text(t),
        Sql::Blob(b) => Value::Blob(b),
    }
}

/// `values` with missing columns as NULL, as SQLite reads a short record.
fn padded(values: &[Value]) -> Vec<Value> {
    let mut values = values.to_vec();
    values.resize(COLUMNS.len(), Value::Null);
    values
}

/// Values SQLite keeps as they are (it reads a stored NaN as NULL), with
/// reals small enough to compare exactly with integers.
fn value() -> impl Strategy<Value = Value> {
    prop_oneof![
        Just(Value::Null),
        any::<i64>().prop_map(Value::Int),
        (-3i64..3).prop_map(Value::Int),
        (-1e15f64..1e15).prop_map(Value::Real),
        (-3i64..3).prop_map(|v| Value::Real(v as f64)),
        any::<String>().prop_map(Value::Text),
        "[a-c]{0,3}".prop_map(Value::Text),
        prop::collection::vec(any::<u8>(), 0..20).prop_map(Value::Blob),
        (0usize..10_000).prop_map(|n| Value::Text("é".repeat(n / 2))),
    ]
}

/// The rows of `t` as SQLite reads them.
fn sqlite_rows(connection: &Connection) -> Vec<(i64, Vec<Value>)> {
    let mut statement = connection
        .prepare("SELECT rowid, a, b, c, d, e, f FROM t ORDER BY rowid")
        .unwrap();
    statement
        .query_map([], |row| {
            let values = (1..=COLUMNS.len())
                .map(|i| row.get::<_, Sql>(i).map(from_sql))
                .collect::<Result<Vec<_>, _>>()?;
            Ok((row.get(0)?, values))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn integrity(connection: &Connection) -> Vec<String> {
    let mut statement = connection.prepare("PRAGMA integrity_check").unwrap();
    statement
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

/// Writes `rows` to table `t` with indexes on `a` and on `(a, b)`, and
/// copies the file to disk.
fn write_with_indexes(rows: &[(i64, Vec<Value>)], file: &TempFile) {
    let storage = Memory::default();
    let mut writer = Writer::new(&storage, "db");
    let table = writer
        .table(rows.iter().map(|(id, v)| (*id, encode_record(v))))
        .unwrap();
    let mut index = |width: usize| {
        let mut entries: Vec<Vec<u8>> = rows
            .iter()
            .map(|(id, values)| {
                let mut key = padded(values)[..width].to_vec();
                key.push(Value::Int(*id));
                encode_record(&key)
            })
            .collect();
        entries.sort_by(|a, b| compare_records(a, b));
        writer.index(entries).unwrap()
    };
    let (by_a, by_ab) = (index(1), index(2));
    let schema = [
        table_entry("t", &COLUMNS, table),
        index_entry("t_a", "t", &["a"], by_a),
        index_entry("t_ab", "t", &["a", "b"], by_ab),
    ];
    writer.finish(&schema, 42).unwrap();
    std::fs::write(&file.0, storage.get("db").unwrap()).unwrap();
}

/// Rows of one to six values: a table has at least one column, and
/// SQLite takes a record without any value for a corrupt one.
fn rows() -> impl Strategy<Value = Vec<(i64, Vec<Value>)>> {
    prop::collection::btree_set(any::<i64>(), 0..200)
        .prop_flat_map(|ids| {
            let n = ids.len();
            (
                Just(ids),
                prop::collection::vec(prop::collection::vec(value(), 1..=COLUMNS.len()), n),
            )
        })
        .prop_map(|(ids, rows)| ids.into_iter().zip(rows).collect())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(48)))]

    /// SQLite finds nothing wrong with what was written and reads the
    /// same rows; its indexes agree with the table.
    #[test]
    fn written_files_pass_sqlites_integrity_check(rows in rows()) {
        let file = TempFile::new("written.db");
        write_with_indexes(&rows, &file);
        let connection = Connection::open(&file.0).unwrap();
        prop_assert_eq!(integrity(&connection), ["ok"]);
        let version: u32 = connection
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        prop_assert_eq!(version, 42);
        let expected: Vec<(i64, Vec<Value>)> =
            rows.iter().map(|(id, v)| (*id, padded(v))).collect();
        prop_assert_eq!(sqlite_rows(&connection), expected);
    }

    /// Rows SQLite stores are read back as SQLite has them.
    #[test]
    fn files_sqlite_wrote_are_read(rows in rows()) {
        let file = TempFile::new("sqlite.db");
        {
            let connection = Connection::open(&file.0).unwrap();
            connection
                .execute_batch("CREATE TABLE t(a, b, c, d, e, f); BEGIN")
                .unwrap();
            for (id, values) in &rows {
                let values = padded(values);
                let mut params: Vec<Sql> = vec![Sql::Integer(*id)];
                params.extend(values.iter().map(to_sql));
                connection
                    .execute(
                        "INSERT INTO t(rowid, a, b, c, d, e, f) VALUES (?, ?, ?, ?, ?, ?, ?)",
                        rusqlite::params_from_iter(params),
                    )
                    .unwrap();
            }
            connection.execute_batch("COMMIT").unwrap();
        }
        let storage = Memory::with("db", std::fs::read(&file.0).unwrap());
        let db = Database::open(&storage, "db").unwrap();
        let (columns, root) = db.table("t").unwrap();
        prop_assert_eq!(columns, COLUMNS.map(String::from).to_vec());
        let mut read = Vec::new();
        db.scan(root, &mut |id, values| {
            read.push((id, padded(values)));
            true
        })
        .unwrap();
        let expected: Vec<(i64, Vec<Value>)> =
            rows.iter().map(|(id, v)| (*id, padded(v))).collect();
        prop_assert_eq!(read, expected);
    }

    /// Single values sort as SQLite's `ORDER BY` sorts them.
    #[test]
    fn records_sort_like_sqlite(values in prop::collection::vec(value(), 0..40)) {
        prop_assert_eq!(our_order(&values), sqlite_order(&values));
    }
}

/// The positions of `values` in our order (ties by position).
fn our_order(values: &[Value]) -> Vec<usize> {
    let records: Vec<Vec<u8>> = values
        .iter()
        .map(|v| encode_record(std::slice::from_ref(v)))
        .collect();
    let mut order: Vec<usize> = (0..values.len()).collect();
    order.sort_by(|&x, &y| compare_records(&records[x], &records[y]).then(x.cmp(&y)));
    order
}

fn sqlite_order(values: &[Value]) -> Vec<usize> {
    let connection = Connection::open_in_memory().unwrap();
    connection.execute_batch("CREATE TABLE s(i, v)").unwrap();
    for (i, v) in values.iter().enumerate() {
        connection
            .execute("INSERT INTO s VALUES (?, ?)", (i as i64, to_sql(v)))
            .unwrap();
    }
    let mut statement = connection.prepare("SELECT i FROM s ORDER BY v, i").unwrap();
    statement
        .query_map([], |r| r.get::<_, i64>(0).map(|i| i as usize))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

/// Large integers next to reals: SQLite compares them exactly, not as
/// two floats.
#[test]
fn large_integers_and_reals_sort_like_sqlite() {
    let big = 1i64 << 53;
    let values = [
        Value::Int(big + 1),
        Value::Real(big as f64),
        Value::Int(big),
    ];
    assert_eq!(our_order(&values), sqlite_order(&values));
    let a = encode_record(&[Value::Int(big + 1)]);
    let b = encode_record(&[Value::Real(big as f64)]);
    assert_eq!(compare_records(&a, &b), Ordering::Greater);
}

/// An index whose entries are long: interior pages hold few of them,
/// the tree is several levels deep and entries reach overflow pages.
#[test]
fn deep_indexes_with_long_entries_pass_the_integrity_check() {
    let rows: Vec<(i64, Vec<Value>)> = (1..=1500)
        .map(|i: i64| {
            let long = if i % 97 == 0 { 6000 } else { 900 };
            let key = format!("{:05}{}", (i * 7919) % 1500, "k".repeat(long));
            (i, vec![text(&key), Value::Int(i)])
        })
        .collect();
    let file = TempFile::new("deep-index.db");
    write_with_indexes(&rows, &file);
    let connection = Connection::open(&file.0).unwrap();
    assert_eq!(integrity(&connection), ["ok"]);
    // A lookup through the index finds the row.
    let key = format!("{:05}{}", (700 * 7919) % 1500, "k".repeat(900));
    let id: i64 = connection
        .query_row(
            "SELECT rowid FROM t INDEXED BY t_a WHERE a = ?",
            [key],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(id, 700);
}

/// An empty table and an empty index are valid too.
#[test]
fn empty_tables_and_indexes_pass_the_integrity_check() {
    let file = TempFile::new("empty.db");
    write_with_indexes(&[], &file);
    let connection = Connection::open(&file.0).unwrap();
    assert_eq!(integrity(&connection), ["ok"]);
    assert_eq!(sqlite_rows(&connection), []);
}

/// Rows with long texts, some deleted again (free pages), for a database
/// file `prepare` sets up first.
fn sqlite_file(file: &TempFile, prepare: &str) -> Vec<(i64, String)> {
    let connection = Connection::open(&file.0).unwrap();
    connection.execute_batch(prepare).unwrap();
    connection
        .execute_batch("CREATE TABLE t(a, b); CREATE INDEX t_b ON t(b); BEGIN")
        .unwrap();
    let mut kept = Vec::new();
    for i in 1..=400i64 {
        let b = format!("{i}:{}", "w".repeat((i as usize * 37) % 3000));
        connection
            .execute(
                "INSERT INTO t(rowid, a, b) VALUES (?, ?, ?)",
                (i, i * 2, &b),
            )
            .unwrap();
        kept.push((i, b));
    }
    connection.execute_batch("COMMIT").unwrap();
    connection
        .execute("DELETE FROM t WHERE rowid % 3 = 0", [])
        .unwrap();
    kept.retain(|(i, _)| i % 3 != 0);
    kept
}

fn read_back(file: &TempFile) -> Vec<(i64, String)> {
    let storage = Memory::with("db", std::fs::read(&file.0).unwrap());
    let db = Database::open(&storage, "db").unwrap();
    let (_, root) = db.table("t").unwrap();
    let mut rows = Vec::new();
    db.scan(root, &mut |id, values| {
        assert_eq!(values[0], Value::Int(id * 2));
        rows.push((id, values[1].as_text().unwrap().to_string()));
        true
    })
    .unwrap();
    rows
}

#[test]
fn every_page_size_sqlite_writes_is_read() {
    for size in [512, 1024, 2048, 4096, 8192, 16_384, 32_768, 65_536] {
        let file = TempFile::new("pages.db");
        let kept = sqlite_file(&file, &format!("PRAGMA page_size = {size};"));
        let bytes = std::fs::read(&file.0).unwrap();
        let stored = u16::from_be_bytes([bytes[16], bytes[17]]);
        assert_eq!(stored, if size == 65_536 { 1 } else { size as u16 });
        assert_eq!(read_back(&file), kept, "page size {size}");
    }
}

#[test]
fn a_wal_database_after_its_checkpoint_is_read() {
    let file = TempFile::new("wal.db");
    let kept = sqlite_file(&file, "PRAGMA journal_mode = WAL;");
    // Closed: checkpointed into the database file.
    let bytes = std::fs::read(&file.0).unwrap();
    assert_eq!(&bytes[18..20], &[2, 2], "a WAL database");
    assert_eq!(read_back(&file), kept);
}

#[test]
fn an_auto_vacuum_database_is_read() {
    let file = TempFile::new("vacuum.db");
    let kept = sqlite_file(&file, "PRAGMA auto_vacuum = FULL;");
    assert_eq!(read_back(&file), kept);
    let file = TempFile::new("incremental.db");
    let kept = sqlite_file(&file, "PRAGMA auto_vacuum = INCREMENTAL;");
    assert_eq!(read_back(&file), kept);
}

#[test]
fn utf16_databases_are_refused() {
    for encoding in ["UTF-16le", "UTF-16be"] {
        let file = TempFile::new("utf16.db");
        {
            let connection = Connection::open(&file.0).unwrap();
            connection
                .execute_batch(&format!(
                    "PRAGMA encoding = '{encoding}'; CREATE TABLE t(a); INSERT INTO t VALUES ('x');"
                ))
                .unwrap();
        }
        let storage = Memory::with("db", std::fs::read(&file.0).unwrap());
        assert_eq!(Database::open(&storage, "db").err(), Some(Error::NotSqlite));
    }
}

/// The tables of KOReader's statistics as KOReader makes them.
#[test]
fn koreader_statistics_are_read() {
    let file = TempFile::new("statistics.sqlite3");
    {
        let connection = Connection::open(&file.0).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS book
                (
                    id integer PRIMARY KEY autoincrement,
                    title text,
                    authors text,
                    notes integer,
                    last_open integer,
                    highlights integer,
                    pages integer,
                    series text,
                    language text,
                    md5 text,
                    total_read_time integer,
                    total_read_pages integer
                );
                CREATE TABLE IF NOT EXISTS page_stat_data
                (
                    id_book integer,
                    page integer NOT NULL DEFAULT 0,
                    start_time integer NOT NULL DEFAULT 0,
                    duration integer NOT NULL DEFAULT 0,
                    total_pages integer NOT NULL DEFAULT 0,
                    UNIQUE (id_book, page, start_time),
                    FOREIGN KEY(id_book) REFERENCES book(id)
                );
                CREATE INDEX IF NOT EXISTS page_stat_data_start_time ON page_stat_data (start_time);
                CREATE VIEW IF NOT EXISTS page_stat AS SELECT id_book FROM page_stat_data;
                PRAGMA user_version = 20221111;
                INSERT INTO book VALUES (NULL, 'Alice', 'Lewis Carroll', 0, 1760000000, 2, 120, 'N/A', 'en', '0123456789abcdef0123456789abcdef', 3600, 40);
                INSERT INTO page_stat_data VALUES (1, 1, 1760000000, 30, 120);
                INSERT INTO page_stat_data VALUES (1, 2, 1760000030, 45, 120);",
            )
            .unwrap();
    }
    let storage = Memory::with("db", std::fs::read(&file.0).unwrap());
    let db = Database::open(&storage, "db").unwrap();
    assert_eq!(db.user_version, 20_221_111);
    let kinds: Vec<(String, String)> = db
        .schema()
        .unwrap()
        .into_iter()
        .map(|e| (e.kind, e.name))
        .collect();
    for wanted in [
        ("table", "book"),
        ("table", "sqlite_sequence"),
        ("table", "page_stat_data"),
        ("index", "page_stat_data_start_time"),
        ("view", "page_stat"),
    ] {
        assert!(
            kinds.contains(&(wanted.0.to_string(), wanted.1.to_string())),
            "{wanted:?} in {kinds:?}"
        );
    }
    let (columns, root) = db.table("book").unwrap();
    assert_eq!(columns.len(), 12);
    assert_eq!(columns[0], "id");
    assert_eq!(columns[11], "total_read_pages");
    let rows = read_rows(&db, root).unwrap();
    assert_eq!(rows.len(), 1);
    // The row id is the book's id; the column itself is stored as NULL.
    assert_eq!(rows[0].0, 1);
    let values = paperpaw_sqlite::decode_record(&rows[0].1).unwrap();
    assert_eq!(values[0], Value::Null);
    assert_eq!(values[1], text("Alice"));
    let (columns, root) = db.table("page_stat_data").unwrap();
    assert_eq!(
        columns,
        ["id_book", "page", "start_time", "duration", "total_pages"]
    );
    assert_eq!(read_rows(&db, root).unwrap().len(), 2);
}
