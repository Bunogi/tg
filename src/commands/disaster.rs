use crate::{include_sql, params, telegram::Telegram, Context};
use chrono::prelude::*;
use darkredis::{Command, CommandList, Value};
use futures::TryStreamExt;
use serde::{Deserialize, Serialize};
use tokio_postgres::types::Type;

#[derive(Serialize, Deserialize)]
struct LastDisaster {
    from: i64,
    to: i64,
    utc: i64,
}

pub async fn add_point(
    receiverid: i64,
    giverid: i64,
    chatid: i64,
    messageid: i64,
    timestamp: i64,
    telegram: &Telegram,
    context: &Context,
) -> Result<(), String> {
    if giverid == receiverid {
        return telegram
            .send_message_silent(chatid, "Cannot give a disaster point to yourself!".into())
            .await
            .map(|_| ())
            .map_err(|e| format!("sending error message: {}", e));
    }

    //Check if the user is on cooldown for giving a point
    let mut redis = context.redis_pool.get().await;
    let cooldown_key = format!("tg.disastercooldown.{}.{}", chatid, giverid);
    let status = redis
        .get(&cooldown_key)
        .await
        .map_err(|e| format!("getting cooldown status: {:?}", e))?;

    //They are on cooldown
    if status.is_some() {
        let ttl_command = Command::new("TTL").arg(&cooldown_key);
        let ttl = redis
            .run_command(ttl_command)
            .await
            .map_err(|e| format!("getting cooldown left: {:?}", e))?
            .unwrap_integer();

        return telegram
            .reply_and_close_keyboard(
                messageid,
                chatid,
                format!(
                    "You can give a new disaster point in {:0.1} hours",
                    crate::util::seconds_to_hours(ttl as i32)
                ),
            )
            .await
            .map(|_| ())
            .map_err(|e| format!("sending error message: {}", e));
    }

    let conn = context.db_pool.get().await.unwrap();
    conn.execute(
        include_sql!("disaster/addpoint.sql"),
        params![chatid, receiverid],
    )
    .await
    .map_err(|e| format!("adding a disaster point: {:?}", e))?;

    //Update last disaster points given list in redis and set cooldown using a pipeline
    let last_disaster_key = format!("tg.lastdisasterpoints.{}", chatid).into_bytes();
    let last_disaster = rmp_serde::to_vec(&LastDisaster {
        from: giverid,
        to: receiverid,
        utc: timestamp,
    })
    .unwrap();
    let timeout = (context.config.disaster.cooldown * 3600).to_string();

    let command = CommandList::new("LPUSH")
        .arg(&last_disaster_key)
        .arg(&last_disaster)
        .command("LTRIM") // Store the last 10
        .arg(&last_disaster_key)
        .arg(b"0")
        .arg(b"9")
        .command("SET")
        .arg(&cooldown_key)
        .arg(b"")
        .arg(b"EX")
        .arg(&timeout); // n hour cooldown

    let res: Result<Vec<darkredis::Value>, darkredis::Error> = redis
        .run_commands(command)
        .await
        .map_err(|e| format!("communicating with redis: {:?}", e))?
        .try_collect()
        .await;

    res.map_err(|e| format!("adding redis disaster data: {:?}", e))?;

    //Send the update status message
    let points: i64 = conn
        .query_one(
            include_sql!("disaster/getuserpoints.sql"),
            params![chatid, receiverid],
        )
        .await
        .map(|r| r.get(0))
        .map_err(|e| format!("getting user points: {:?}", e))?;

    telegram
        .reply_and_close_keyboard(
            messageid,
            chatid,
            format!(
                "{} now has {} disaster points.",
                crate::util::get_user(chatid, receiverid, telegram, &context.config, &mut redis)
                    .await,
                points
            ),
        )
        .await
        .map_err(|e| format!("sending disaster point count: {}", e))?;

    Ok(())
}

pub async fn show_points(
    chatid: i64,
    telegram: &Telegram,
    context: &Context,
) -> Result<(), String> {
    let conn = context.db_pool.get().await.unwrap();
    let stmt = conn
        .prepare_typed(include_sql!("disaster/getchatpoints.sql"), &[Type::INT8])
        .await
        .unwrap();

    let points = conn
        .query(&stmt, params![chatid])
        .await
        .map_err(|e| format!("getting chat points: {:?}", e))?
        .into_iter()
        .map(|row| (row.get(0), row.get(1)))
        .collect::<Vec<(i64, i64)>>();

    if points.is_empty() {
        return telegram
            .send_message_silent(chatid, "No points have been given yet".into())
            .await
            .map(|_| ())
            .map_err(|e| format!("sending no points message: {}", e));
    }

    let mut redis = context.redis_pool.get().await;
    let mut output = "```\n".to_string(); //Monospace the whole output
    for (points, userid) in points {
        //Add user points to output
        let appendage = format!(
            "{}: {}\n",
            crate::util::get_user(chatid, userid, telegram, &context.config, &mut redis).await,
            points
        );

        output += &appendage;
    }

    //Get a list of the last points that were given out
    let last_disaster_key = format!("tg.lastdisasterpoints.{}", chatid).into_bytes();
    let given_points = redis
        .run_command(
            Command::new("LRANGE")
                .arg(&last_disaster_key)
                .arg(b"0")
                .arg(b"9"),
        )
        .await
        .map_err(|e| format!("getting last disaster points given: {:?}", e))?
        .unwrap_array();

    output += "Last sent points:\n";

    //Format these entries nicely
    for value in given_points {
        if let Value::String(v) = value {
            let entry: LastDisaster = rmp_serde::from_slice(&v).unwrap();
            let giver =
                crate::util::get_user(chatid, entry.from, telegram, &context.config, &mut redis)
                    .await;
            let sender =
                crate::util::get_user(chatid, entry.to, telegram, &context.config, &mut redis)
                    .await;

            let utc = DateTime::<Utc>::from_utc(
                NaiveDateTime::from_timestamp_opt(entry.utc, 0).unwrap(),
                Utc,
            );
            let time_string = utc.with_timezone(&Local).format("%e %B %k:%M %:z");

            let appendage = format!("[{}] {} -> {}\n", time_string, giver, sender);
            output += &appendage;
        } else {
            panic!("received invalid datatype from redis")
        }
    }

    telegram
        .send_message_silently_with_markdown(chatid, format!("{}```", output))
        .await
        .map_err(|e| format!("sending disaster point count: {}", e))?;

    Ok(())
}
