use futures::lock::{Mutex, MutexGuard};
use rusqlite::Connection;
use std::sync::Arc;

#[derive(Clone)]
pub struct SqlPool {
    connections: Vec<Arc<Mutex<Connection>>>,
    path: String,
}

impl SqlPool {
    pub fn new(max_connections: usize, path: &str) -> rusqlite::Result<Self> {
        let mut connections = Vec::new();
        let path = path.to_string();
        for _ in 0..max_connections {
            let conn = Arc::new(Mutex::new(rusqlite::Connection::open(&path)?));
            connections.push(conn);
        }
        Ok(Self { connections, path })
    }
    pub async fn get(&self) -> MutexGuard<'_, Connection> {
        for conn in self.connections.iter() {
            if let Some(lock) = conn.try_lock() {
                return lock;
            }
        }

        //No free connections found, get the first available one
        let lockers = self.connections.iter().map(|l| l.lock());
        futures::future::select_all(lockers).await.0
    }
}

#[macro_export]
macro_rules! include_sql {
    ($s:tt) => {
        include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/sql/", $s))
    };
}
