use futures::{lock::Mutex, prelude::*};

use quick_error::quick_error;
use runtime::net::TcpStream;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::io;
use std::sync::Arc;
use std::time;

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

    pub async fn set<'a, S>(&'a mut self, key: &'a str, value: S) -> Result<()>
    where
        S: Serialize,
    {
        let mut stream = self.stream.lock().await;

        let json = serde_json::to_string(&value).unwrap();

        //SET <key> <value>
        let message = format!(
            "*3\r\n$3\r\nSET\r\n${}\r\n{}\r\n${}\r\n{}\r\n",
            key.len(),
            key,
            json.len(),
            json
        );
        let message = message.into_bytes();
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

    pub async fn set_with_expiry<'a, S>(
        &'a mut self,
        key: &'a str,
        value: S,
        expiry: time::Duration,
    ) -> Result<()>
    where
        S: Serialize,
    {
        let mut stream = self.stream.lock().await;

        let json = serde_json::to_string(&value).unwrap();
        let expiry = expiry.as_millis().to_string();

        //SET <key> <value> PX n
        let message = format!(
            "*5\r\n$3\r\nSET\r\n${}\r\n{}\r\n${}\r\n{}\r\n$2\r\nPX\r\n${}\r\n{}\r\n",
            key.len(),
            key,
            json.len(),
            json,
            expiry.len(),
            expiry
        );
        let message = message.into_bytes();
        debug!("Setting key {} to {}", key, json);
        stream.write_all(&message).await?;

        debug!("Reading from stream");
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

        let deserialized = serde_json::from_slice(&buf).unwrap();
        Ok(Some(deserialized))
    }
}
