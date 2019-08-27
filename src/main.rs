#[macro_use]
extern crate log;
#[macro_use]
extern crate rusqlite;

use crate::telegram::Telegram;
use db::SqlPool;
use futures::stream::StreamExt;
use serde::Deserialize;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::process::exit;

mod commands;
mod db;
mod handlers;
mod telegram;
mod util;

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    markov: Markov,
    redis: RedisConfig,
    cache: CacheConfig,
    disaster: DisasterConfig,
    general: GeneralConfig,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct GeneralConfig {
    time_format: String,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct DisasterConfig {
    cooldown: u64,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct CacheConfig {
    username: u64,
    markov_chain: u64,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Markov {
    chain_order: usize,
    max_order: usize,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RedisConfig {
    address: String,
    password: Option<String>,
}

pub struct Context {
    config: Config,
    redis_pool: darkredis::ConnectionPool,
    db_pool: SqlPool,
}

#[runtime::main(runtime_tokio::Tokio)]
async fn main() -> std::io::Result<()> {
    env_logger::init();

    let config_path = Path::new("tg.toml");
    info!(
        "Loading config file '{}'...",
        config_path.canonicalize().unwrap().to_string_lossy()
    );
    let mut file = File::open(config_path)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    let config: Config = match toml::from_str(&contents) {
        Ok(c) => c,
        Err(e) => {
            error!("Couldn't parse config file: {}", e);
            exit(1);
        }
    };
    info!("File loaded!");

    //maximum number of connections to redis and the database
    let max_connections = num_cpus::get();
    info!("Using {} pooled connections", max_connections);
    info!("Opening Redis connections...");
    let address = &config.redis.address;
    let password = &config.redis.password;
    let redis_pool = match darkredis::ConnectionPool::create(
        address.into(),
        password.as_ref().map(|p| &**p), //not pretty but it works
        max_connections,
    )
    .await
    {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to connect to redis: {}", e);
            exit(1);
        }
    };
    info!("Opening database connections...");
    let db_pool = SqlPool::new(max_connections, "logs.db").unwrap();

    {
        info!("Creating tables if necesarry...");
        let db = db_pool.get().await;
        db.execute_batch(include_sql!("create.sql")).unwrap();
        info!("Tables created!");
    }

    info!("Connecting to Telegram...");
    let telegram = Telegram::new(std::env::var("TELEGRAM_BOT_TOKEN").unwrap()).await;
    let context = Context {
        config,
        redis_pool,
        db_pool,
    };

    loop {
        info!("Listening to updates...");
        telegram
            .updates()
            .for_each_concurrent(None, |f| handlers::handle_update(f, &telegram, &context))
            .await;
    }
}
