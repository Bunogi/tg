#[macro_use]
extern crate log;

use crate::telegram::Telegram;
use deadpool_postgres::{Manager, Pool};
use futures::stream::StreamExt;
use serde::Deserialize;
use std::{fs::File, io::Read, path::Path, process::exit};

mod commands;
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
    postgres: PostgresConfig,
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

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct PostgresConfig {
    user: String,
    host: String,
    password: Option<String>,
}

pub struct Context {
    config: Config,
    redis_pool: darkredis::ConnectionPool,
    db_pool: Pool,
}

//HACK: spawn real_main() in a task so that tokio::task::block_in_place works.
//Should be fixed in a later version of tokio but do this for now
#[tokio::main]
async fn main() -> std::io::Result<()> {
    tokio::spawn(real_main()).await.unwrap()
}

async fn real_main() -> std::io::Result<()> {
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
    let mut postgres_config = tokio_postgres::Config::new();
    postgres_config.host(&config.postgres.host);
    postgres_config.user(&config.postgres.user);
    if let Some(ref p) = &config.postgres.password {
        postgres_config.password(p);
    }
    let db_pool = Pool::new(
        Manager::new(postgres_config, tokio_postgres::NoTls),
        max_connections,
    );

    {
        info!("Creating tables if necesarry...");
        let db = db_pool.get().await.unwrap();
        db.batch_execute(include_sql!("create.sql")).await.unwrap();
        info!("Tables created!");
    }

    info!("Connecting to Telegram...");
    let telegram = Telegram::connect(std::env::var("TELEGRAM_BOT_TOKEN").unwrap()).await;
    match telegram {
        Ok(telegram) => {
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
        Err(e) => {
            error!("Failed to connect to telegram: {}", e);
            exit(2);
        }
    }
}
