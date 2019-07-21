use futures::{
    lock::{Mutex, MutexGuard},
    prelude::*,
};
use quick_error::quick_error;
use runtime::net::TcpStream;
use std::io;
use std::sync::Arc;
use std::time;

#[derive(Clone)]
pub struct RedisPool {
    connections: Vec<Arc<Mutex<RedisConnection>>>,
}

impl RedisPool {
    pub async fn create(max_connections: usize) -> Result<Self> {
        let mut connections = Vec::new();
        for i in 0..max_connections {
            let mut conn = RedisConnection::connect().await?;
            conn.run_command(
                Command::new("CLIENT")
                    .arg(b"SETNAME")
                    .arg(&format!("Disastia-Telegram-Bot-{}", i).into_bytes()),
            )
            .await?;
            let conn = Arc::new(Mutex::new(conn));

            connections.push(conn);
        }
        Ok(Self { connections })
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
        UnexpectedResponse(got: String) {
            display("Unexpected Redis response \"{}\"", got)
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

pub struct Command {
    commands: Vec<Vec<Vec<u8>>>,
    amount: usize,
}

impl Command {
    pub fn new(cmd: &str) -> Self {
        let commands = vec![vec![cmd.to_string().into_bytes()]];
        Self {
            commands,
            amount: 1,
        }
    }

    pub fn arg(mut self, bytes: &[u8]) -> Self {
        self.commands.last_mut().unwrap().push(bytes.to_vec());
        self
    }

    pub fn command(mut self, cmd: &str) -> Self {
        self.commands.push(vec![cmd.to_string().into_bytes()]);
        self.amount += 1;
        self
    }

    //Convert to redis protocol encoding
    fn serialize(self) -> Vec<u8> {
        let mut out = Vec::new();
        for command in self.commands {
            let mut this_command = format!("*{}\r\n", command.len()).into_bytes();
            for arg in command {
                let mut serialized = format!("${}\r\n", arg.len()).into_bytes();
                for byte in arg {
                    serialized.push(byte);
                }
                serialized.push(b'\r');
                serialized.push(b'\n');

                this_command.append(&mut serialized);
            }
            out.append(&mut this_command);
        }

        out
    }
}

#[derive(Debug)]
pub enum Value {
    Array(Vec<Value>),
    Integer(isize),
    Nil,
    String(Vec<u8>),
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

    async fn parse_simple_value(buf: &[u8]) -> Result<Value> {
        match buf[0] {
            b'+' => Ok(Value::String(buf[1..].into())),
            b'-' => {
                //TODO: find a way to do this without copying
                Err(Error::RedisError(
                    String::from_utf8_lossy(&buf[1..]).to_string(),
                ))
            }
            b':' => {
                //TODO: find a way to do this without copying
                let string = String::from_utf8_lossy(&buf[1..]);
                let num = string.trim().parse::<isize>().unwrap();
                Ok(Value::Integer(num))
            }
            _ => Err(Error::UnexpectedResponse(
                String::from_utf8_lossy(buf).to_string(),
            )),
        }
    }

    async fn parse_string(start: &[u8], stream: &mut TcpStream) -> Result<Value> {
        if start == b"$-1\r\n" {
            Ok(Value::Nil)
        } else {
            let num = String::from_utf8_lossy(&start[1..])
                .trim()
                .parse::<usize>()
                .unwrap();
            let mut buf = vec![0u8; num + 2]; // add two to catch the final \r\n from redis
            stream.read_exact(&mut buf).await?;
            //TODO: this probably doesn't need to copy either..
            Ok(Value::String(buf[..num].to_vec()))
        }
    }

    //Assumes that there will never be nested arrays in a redis response.
    async fn parse_array(start: &[u8], mut stream: &mut TcpStream) -> Result<Value> {
        let num = String::from_utf8_lossy(&start[1..])
            .trim()
            .parse::<usize>()
            .unwrap();

        let mut values = Vec::with_capacity(num);

        for _ in 0..num {
            let buf = read_until(&mut stream, b'\n').await?;
            match buf[0] {
                b'+' | b'-' | b':' => values.push(Self::parse_simple_value(&buf).await?),
                b'$' => values.push(Self::parse_string(&buf, &mut stream).await?),
                _ => {
                    return Err(Error::UnexpectedResponse(
                        String::from_utf8_lossy(&buf).to_string(),
                    ))
                }
            }
        }

        Ok(Value::Array(values))
    }

    //Read one value from the stream using the parse_* utility functions
    async fn read_value(mut stream: &mut TcpStream) -> Result<Value> {
        let buf = read_until(&mut stream, b'\n').await?;
        match buf[0] {
            b'+' | b'-' | b':' => Self::parse_simple_value(&buf).await,
            b'$' => Self::parse_string(&buf, &mut stream).await,
            b'*' => Self::parse_array(&buf, &mut stream).await,
            _ => Err(Error::UnexpectedResponse(
                String::from_utf8_lossy(&buf).to_string(),
            )),
        }
    }

    pub async fn run_command(&mut self, command: Command) -> Result<Vec<Value>> {
        let mut stream = self.stream.lock().await;
        let number_of_commands = command.amount;
        let serialized = command.serialize();
        stream.write_all(&serialized).await?;

        let mut results = Vec::with_capacity(number_of_commands);
        for _ in 0..number_of_commands {
            results.push(Self::read_value(&mut stream).await?);
        }

        Ok(results)
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
            Err(Error::UnexpectedResponse(format!(
                "{}",
                String::from_utf8_lossy(&buf)
            )))
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
            Err(Error::UnexpectedResponse(format!(
                "{}",
                String::from_utf8_lossy(&buf)
            )))
        } else {
            Ok(())
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
                ))
            }
        }

        Ok(Some(buf))
    }
}
