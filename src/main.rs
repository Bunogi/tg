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
mod telegram;
mod util;

#[runtime::main(runtime_tokio::Tokio)]
async fn main() -> std::io::Result<()> {
    env_logger::init();
    //maximum number of connections to redis and the database
    let max_connections = num_cpus::get();
    info!("Using {} pooled connections", max_connections);
    info!("Opening Redis connections...");
    let redis_pool =
        match darkredis::ConnectionPool::create("127.0.0.1:6379".into(), None, max_connections)
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

    let telegram = Telegram::new(std::env::var("TELEGRAM_BOT_TOKEN").unwrap()).await;

    info!("Startup complete, listening for updates...");
    loop {
        telegram
            .updates()
            .for_each_concurrent(None, |f| {
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
