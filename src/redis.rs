use futures::{lock::Mutex, prelude::*};

use runtime::net::TcpStream;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::io;
use std::sync::Arc;

#[derive(Clone)]
pub struct RedisConnection {
    stream: Arc<Mutex<TcpStream>>,
}

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
    pub async fn new() -> io::Result<Self> {
        let stream = Arc::new(Mutex::new(TcpStream::connect("127.0.0.1:6379").await?));
        Ok(Self { stream })
    }

    //TODO: burn the following functions in hell and do it properly
    pub async fn set<'a, S>(&'a mut self, key: &'a str, value: S) -> io::Result<()>
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

        let buf = read_until(&mut stream, '\n' as u8).await?;
        if buf != b"+OK\r\n" {
            panic!(
                "unexpected redis response {:?}",
                String::from_utf8_lossy(&buf)
            )
        } else {
            Ok(())
        }
    }

    pub async fn get<'a, D>(&'a mut self, key: &'a str) -> Result<Option<D>, io::Error>
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
                buf = read_until(&mut stream, '\n' as u8).await?;
            }
            '$' => {
                buf = read_until(&mut stream, '\n' as u8).await?;
                let string = String::from_utf8(buf).unwrap();
                let len = string.trim().parse::<usize>().unwrap();
                buf = vec![0; len];
                stream.read_exact(&mut buf).await?;
            }
            '-' => {
                buf = read_until(&mut stream, '\n' as u8).await?;
                panic!("Got error message: {}", String::from_utf8_lossy(&buf));
            }
            _ => panic!(
                "Unexpected redis response: {:?}",
                String::from_utf8_lossy(&buf)
            ),
        }

        if &buf[0..3] == b"nil" {
            Ok(None)
        } else {
            let deserialized = serde_json::from_slice(&buf).unwrap();
            Ok(Some(deserialized))
        }
    }
}
