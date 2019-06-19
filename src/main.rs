#![feature(async_await)]

#[macro_use]
extern crate log;
#[macro_use]
extern crate rusqlite;

use crate::telegram::Telegram;
use db::AsyncSqlConnection;
use futures::stream::StreamExt;
use rusqlite::Connection;
use std::process::exit;

mod commands;
mod db;
mod handlers;
mod redis;
mod telegram;
mod util;

#[runtime::main(runtime_tokio::Tokio)]
async fn main() -> std::io::Result<()> {
    env_logger::init();
    info!("Connecting to redis...");
    let redis_conn = match redis::RedisConnection::connect().await {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to connect to redis: {}", e);
            exit(1);
        }
    };
    info!("Opening database...");
    let db_conn = AsyncSqlConnection::new(Connection::open("logs.db").unwrap());

    {
        info!("Creating tables if necesarry...");
        let db = db_conn.get().await;
        db.execute_batch(include_sql!("create.sql")).unwrap();
        info!("Done!");
    }

    let telegram = Telegram::new(std::env::var("TELEGRAM_BOT_TOKEN").unwrap()).await;

    info!("Running bot...");
    loop {
        telegram
            .updates()
            .for_each(|f| {
                runtime::spawn(handlers::handle_update(
                    telegram.clone(),
                    f,
                    redis_conn.clone(),
                    db_conn.clone(),
                ))
            })
            .await;
        info!("BOT: Stream ended, reconnecting");
    }
}
