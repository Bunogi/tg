#![feature(async_await, unsized_locals)]

#[macro_use]
extern crate log;
#[macro_use]
extern crate rusqlite;

use crate::telegram::Telegram;
use db::SqlPool;
use futures::stream::StreamExt;
use std::process::exit;

mod commands;
mod db;
mod handlers;
mod redis;
mod telegram;
mod util;

#[runtime::main(runtime_tokio::Tokio)]
async fn main() -> std::io::Result<()> {
    //maximum number of connections to redis and the database
    const MAX_CONNECTIONS: usize = 4;
    env_logger::init();
    info!("Opening redis connections...");
    let redis_pool = match redis::RedisPool::new(MAX_CONNECTIONS).await {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to connect to redis: {}", e);
            exit(1);
        }
    };
    info!("Opening database connections...");
    let db_pool = SqlPool::new(MAX_CONNECTIONS, "logs.db").unwrap();

    {
        info!("Creating tables if necesarry...");
        let db = db_pool.get().await;
        db.execute_batch(include_sql!("create.sql")).unwrap();
        info!("Tables created!");
    }

    let telegram = Telegram::new(std::env::var("TELEGRAM_BOT_TOKEN").unwrap()).await;

    info!("Startup complete, listening for updates...");
    loop {
        telegram
            .updates()
            .for_each(|f| {
                runtime::spawn(handlers::handle_update(
                    telegram.clone(),
                    f,
                    redis_pool.clone(),
                    db_pool.clone(),
                ))
            })
            .await;
        info!("BOT: Stream ended, reconnecting");
    }
}
