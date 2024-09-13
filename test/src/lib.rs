use limbo_core::Connection;
use limbo_core::Database;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

struct TempDatabase {
    pub path: PathBuf,
    pub io: Arc<dyn limbo_core::IO>,
}

impl TempDatabase {
    pub fn new(table_sql: &str) -> Self {
        let mut path = env::current_dir().unwrap();
        path.push("test.db");
        {
            if path.exists() {
                fs::remove_file(&path).unwrap();
            }
            let connection = rusqlite::Connection::open(&path).unwrap();
            connection.execute(table_sql, ()).unwrap();
        }
        let io: Arc<dyn limbo_core::IO> = Arc::new(limbo_core::PlatformIO::new().unwrap());

        Self { path, io }
    }

    pub fn connect_limbo(&self) -> limbo_core::Connection {
        let db = Database::open_file(self.io.clone(), self.path.to_str().unwrap()).unwrap();
        let conn = db.connect();
        conn
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use limbo_core::{Database, RowResult, Value};
    use std::sync::Arc;

    #[test]
    fn test_sequential_write() -> anyhow::Result<()> {
        env_logger::init();

        let tmp_db = TempDatabase::new("CREATE TABLE test (x INTEGER PRIMARY KEY);");
        let conn = tmp_db.connect_limbo();

        let list_query = "SELECT * FROM test";
        let max_iterations = 10000;
        for i in 0..max_iterations {
            if (i % 100) == 0 {
                let progress = (i as f64 / max_iterations as f64) * 100.0;
                println!("progress {:.1}%", progress);
            }
            let insert_query = format!("INSERT INTO test VALUES ({})", i);
            match conn.query(insert_query) {
                Ok(Some(ref mut rows)) => loop {
                    match rows.next_row()? {
                        RowResult::IO => {
                            tmp_db.io.run_once()?;
                        }
                        RowResult::Done => break,
                        _ => unreachable!(),
                    }
                },
                Ok(None) => {}
                Err(err) => {
                    eprintln!("{}", err);
                }
            };

            let mut current_read_index = 0;
            match conn.query(list_query) {
                Ok(Some(ref mut rows)) => loop {
                    match rows.next_row()? {
                        RowResult::Row(row) => {
                            let first_value = row.values.first().expect("missing id");
                            let id = match first_value {
                                Value::Integer(i) => *i as i32,
                                Value::Float(f) => *f as i32,
                                _ => unreachable!(),
                            };
                            assert_eq!(current_read_index, id);
                            current_read_index += 1;
                        }
                        RowResult::IO => {
                            tmp_db.io.run_once()?;
                        }
                        RowResult::Done => break,
                    }
                },
                Ok(None) => {}
                Err(err) => {
                    eprintln!("{}", err);
                }
            }
            conn.cacheflush()?;
        }
        Ok(())
    }

    #[test]
    fn test_simple_overflow_page() -> anyhow::Result<()> {
        env_logger::init();
        let tmp_db = TempDatabase::new("CREATE TABLE test (x INTEGER PRIMARY KEY, t TEXT);");
        let conn = tmp_db.connect_limbo();

        let mut huge_text = String::new();
        for i in 0..8192 {
            huge_text.push(('A' as u8 + (i % 24) as u8) as char);
        }

        let list_query = "SELECT * FROM test LIMIT 1";
        let insert_query = format!("INSERT INTO test VALUES (1, '{}')", huge_text.as_str());

        match conn.query(insert_query) {
            Ok(Some(ref mut rows)) => loop {
                match rows.next_row()? {
                    RowResult::IO => {
                        tmp_db.io.run_once()?;
                    }
                    RowResult::Done => break,
                    _ => unreachable!(),
                }
            },
            Ok(None) => {}
            Err(err) => {
                eprintln!("{}", err);
            }
        };

        match conn.query(list_query) {
            Ok(Some(ref mut rows)) => loop {
                match rows.next_row()? {
                    RowResult::Row(row) => {
                        let first_value = &row.values[0];
                        let text = &row.values[1];
                        let id = match first_value {
                            Value::Integer(i) => *i as i32,
                            Value::Float(f) => *f as i32,
                            _ => unreachable!(),
                        };
                        let text = match text {
                            Value::Text(t) => *t,
                            _ => unreachable!(),
                        };
                        assert_eq!(1, id);
                        assert_eq!(*text, huge_text);
                    }
                    RowResult::IO => {
                        tmp_db.io.run_once()?;
                    }
                    RowResult::Done => break,
                }
            },
            Ok(None) => {}
            Err(err) => {
                eprintln!("{}", err);
            }
        }
        conn.cacheflush()?;
        Ok(())
    }
}
