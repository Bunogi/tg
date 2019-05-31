use futures::lock::{Mutex, MutexGuard};
use rusqlite::Connection;
use std::sync::Arc;

#[derive(Clone)]
pub struct AsyncSqlConnection {
    inner: Arc<Mutex<Connection>>,
}

impl AsyncSqlConnection {
    pub fn new(conn: Connection) -> Self {
        Self {
            inner: Arc::new(Mutex::new(conn)),
        }
    }
    pub async fn get(&self) -> MutexGuard<Connection> {
        self.inner.lock().await
    }
}

#[macro_export]
macro_rules! include_sql {
    ($s:tt) => {
        include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/sql/", $s))
    };
}
