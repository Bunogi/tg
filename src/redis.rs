use futures::{lock::{Mutex, MutexGuard}, prelude::*};

use quick_error::quick_error;
use runtime::net::TcpStream;
use serde::de::DeserializeOwned;
use std::io;
use std::sync::Arc;
use std::time;

#[derive(Clone)]
pub struct RedisPool {
    connections: Vec<Arc<Mutex<RedisConnection>>>
}

impl RedisPool {
    pub async fn new(max_connections: usize) -> Result<Self> {
        let mut connections = Vec::new();
        for _ in 0..max_connections {
            let conn = Arc::new(Mutex::new(RedisConnection::connect().await?));
            connections.push(conn);
        }
        Ok(Self { connections})
    }

    pub async fn get(&self) -> MutexGuard<RedisConnection> {
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

#[derive(Clone)]
pub struct RedisConnection {
    address: String,
    stream: Arc<Mutex<TcpStream>>,
}

quick_error! {
    #[derive(Debug)]
    pub enum Error {
        Io(err: io::Error) {
            from()
        }
        ConnectionFailed(err: io::Error) {}
        UnexpectedResponse(got: String, expected: String) {
            display("Unexpected Redis response \"{}\", expected {}", got, expected)
        }
        RedisError(err: String) {
            display("Redis replied with error: {}", err)
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

async fn read_until(r: &mut TcpStream, byte: u8) -> io::Result<Vec<u8>> {
    let mut buffer = Vec::new();
    let mut single = [0; 1];
    loop {
        r.read(&mut single).await?;
        buffer.push(single[0]);
        if single[0] == byte {
            return Ok(buffer);
        }
    }
}

impl RedisConnection {
    pub async fn connect() -> Result<Self> {
        let address = "127.0.0.1:6379";
        let stream = Self::reconnect(&address).await?;
        Ok(Self {
            address: address.into(),
            stream,
        })
    }

    async fn reconnect(address: &str) -> Result<Arc<Mutex<TcpStream>>> {
        Ok(Arc::new(Mutex::new(
            TcpStream::connect(address)
                .await
                .map_err(Error::ConnectionFailed)?,
        )))
    }

    pub async fn set<'a, D>(&'a mut self, key: &'a str, data: D) -> Result<()>
    where
        D: AsRef<[u8]>,
    {
        let mut stream = self.stream.lock().await;

        let data = data.as_ref();

        //SET <key> <value>
        let mut message = format!(
            "*3\r\n$3\r\nSET\r\n${}\r\n{}\r\n${}\r\n",
            key.len(),
            key,
            data.len(),
        )
        .into_bytes();

        for d in data.iter().cloned() {
            message.push(d);
        }

        message.push(b'\r');
        message.push(b'\n');

        stream.write_all(&message).await?;

        let buf = read_until(&mut stream, b'\n').await?;
        if buf != b"+OK\r\n" {
            Err(Error::UnexpectedResponse(
                "+OK\r\n".into(),
                format!("{}", String::from_utf8_lossy(&buf)),
            ))
        } else {
            Ok(())
        }
    }

    pub async fn set_with_expiry<'a, D>(
        &'a mut self,
        key: &'a str,
        data: D,
        expiry: time::Duration,
    ) -> Result<()>
    where
        D: AsRef<[u8]>,
    {
        let mut stream = self.stream.lock().await;

        let expiry = expiry.as_millis().to_string();
        let data = data.as_ref();

        //SET <key> <value> PX n
        let mut message = format!(
            "*5\r\n$3\r\nSET\r\n${}\r\n{}\r\n${}\r\n",
            key.len(),
            key,
            data.len(),
        )
        .into_bytes();

        for d in data.iter().cloned() {
            message.push(d);
        }

        let message: Vec<u8> = message
            .into_iter()
            .chain(
                format!("\r\n$2\r\nPX\r\n${}\r\n{}\r\n", expiry.len(), expiry)
                    .into_bytes()
                    .into_iter(),
            )
            .collect();

        stream.write_all(&message).await?;

        let buf = read_until(&mut stream, b'\n').await?;
        if buf != b"+OK\r\n" {
            Err(Error::UnexpectedResponse(
                "+OK\r\n".into(),
                format!("{}", String::from_utf8_lossy(&buf)),
            ))
        } else {
            Ok(())
        }
    }

    pub async fn get<'a, D>(&'a mut self, key: &'a str) -> Result<Option<D>>
    where
        D: DeserializeOwned,
    {
        let buf = self.get_bytes(key).await?;
        if let Some(buf) = buf {
            let deserialized = serde_json::from_slice(&buf).unwrap();
            Ok(Some(deserialized))
        } else {
            Ok(None)
        }
    }

    pub async fn get_bytes<'a>(&'a mut self, key: &'a str) -> Result<Option<Vec<u8>>> {
        let mut stream = self.stream.lock().await;
        //GET <key>
        let message = format!("*2\r\n$3\r\nGET\r\n${}\r\n{}\r\n", key.len(), key);
        stream.write_all(message.as_bytes()).await?;

        //read one byte
        let mut buf = vec![0; 1];
        stream.read(&mut buf).await?;
        match buf[0] as char {
            '+' => {
                buf = read_until(&mut stream, b'\n').await?;
            }
            '$' => {
                buf = read_until(&mut stream, b'\n').await?;
                let string = String::from_utf8(buf).unwrap();
                //nil
                if &string == "-1\r\n" {
                    return Ok(None);
                }

                let len = string.trim().parse::<usize>().unwrap();
                buf = vec![0; len + 2]; //read trailing \r\n from stream so we don't read it later
                stream.read_exact(&mut buf).await?;
            }
            '-' => {
                buf = read_until(&mut stream, b'\n').await?;
                return Err(Error::RedisError(String::from_utf8_lossy(&buf).into()));
            }
            _ => {
                return Err(Error::UnexpectedResponse(
                    String::from_utf8_lossy(&buf).into(),
                    "Non-error reply".into(),
                ))
            }
        }

        Ok(Some(buf))
    }
}
