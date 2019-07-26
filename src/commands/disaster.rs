use crate::{
    db::SqlPool,
    include_sql,
    telegram::{message::Message, Telegram},
};
use chrono::prelude::*;
use darkredis::{Command, CommandList, Value};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct LastDisaster {
    from: i64,
    to: i64,
    utc: i64,
}

pub async fn add_point(
    userid: i64,
    message: &Message,
    context: Telegram,
    sql_pool: SqlPool,
    redis_pool: darkredis::ConnectionPool,
) -> Result<(), String> {
    if message.from.id as i64 == userid {
        return context
            .send_message_silent(
                message.chat.id,
                "Cannot give a disaster point to yourself!".into(),
            )
            .await
            .map(|_| ())
            .map_err(|e| format!("sending error message: {:?}", e));
    }

    //Check if the user is on cooldown for giving a point
    let mut redis = redis_pool.get().await;
    let cooldown_key = format!(
        "tg.disastercooldown.{}.{}",
        message.chat.id, message.from.id
    );
    let status = redis
        .get(&cooldown_key)
        .await
        .map_err(|e| format!("getting cooldown status: {:?}", e))?;

    //They are on cooldown
    if status.is_some() {
        let ttl_command = Command::new("TTL").arg(cooldown_key.as_bytes());
        let ttl = redis
            .run_command(ttl_command)
            .await
            .map_err(|e| format!("getting cooldown left: {:?}", e))?
            .unwrap_integer();

        return context
            .reply_and_close_keyboard(
                message.id,
                message.chat.id,
                format!(
                    "You can give a new disaster point in {:0.1} hours",
                    crate::util::seconds_to_hours(ttl as i32)
                ),
            )
            .await
            .map(|_| ())
            .map_err(|e| format!("sending error message: {:?}", e));
    }

    let conn = sql_pool.get().await;
    conn.execute(
        include_sql!("disaster/addpoint.sql"),
        params![message.chat.id, userid],
    )
    .map_err(|e| format!("adding a point: {:?}", e))?;

    //Update last disaster points given list in redis and set cooldown using a pipeline
    let last_disaster_key = format!("tg.lastdisasterpoints.{}", message.chat.id).into_bytes();
    let command = CommandList::new("LPUSH")
        .arg(&last_disaster_key)
        .arg(
            &rmp_serde::to_vec(&LastDisaster {
                from: message.from.id as i64,
                to: userid,
                utc: message.date as i64,
            })
            .unwrap(),
        )
        .command("LTRIM") // Store the last 10
        .arg(&last_disaster_key)
        .arg(b"0")
        .arg(b"9")
        .command("SET")
        .arg(&cooldown_key.into_bytes())
        .arg(b"")
        .arg(b"EX")
        .arg((3 * 60 * 60).to_string().as_bytes()); // 3 hour cooldown

    redis
        .run_commands(command)
        .await
        .map_err(|e| format!("adding redis disaster data: {:?}", e))?;

    //Send the update status message
    let points: isize = conn
        .query_row(
            include_sql!("disaster/getuserpoints.sql"),
            params![message.chat.id, userid],
            |row| Ok(row.get(0)?),
        )
        .map_err(|e| format!("getting user points: {:?}", e))?;

    context
        .reply_and_close_keyboard(
            message.id,
            message.chat.id,
            format!(
                "{} now has {} disaster points.",
                crate::util::get_user(message.chat.id, userid, context.clone(), redis.clone())
                    .await,
                points
            ),
        )
        .await
        .map_err(|e| format!("sending disaster point count: {:?}", e))?;

    Ok(())
}

pub async fn show_points(
    chatid: i64,
    context: Telegram,
    sql_pool: SqlPool,
    redis_pool: darkredis::ConnectionPool,
) -> Result<(), String> {
    let conn = sql_pool.get().await;
    let points = conn
        .prepare_cached(include_sql!("disaster/getchatpoints.sql"))
        .unwrap()
        .query_map(params![chatid], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<Vec<(i32, i64)>, rusqlite::Error>>()
        .map_err(|e| format!("getting chat points: {:?}", e))?;

    if points.is_empty() {
        return context
            .send_message_silent(chatid, "No points have been given yet".into())
            .await
            .map(|_| ())
            .map_err(|e| format!("sending no points message: {:?}", e));
    }

    let mut redis = redis_pool.get().await;
    let mut output = String::new();
    //Add user points to output
    for (points, userid) in points {
        let appendage = format!(
            "{}: {}\n",
            crate::util::get_user(chatid, userid, context.clone(), redis.clone()).await,
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
                crate::util::get_user(chatid, entry.from, context.clone(), redis.clone()).await;
            let sender =
                crate::util::get_user(chatid, entry.to, context.clone(), redis.clone()).await;

            let utc = DateTime::<Utc>::from_utc(NaiveDateTime::from_timestamp(entry.utc, 0), Utc);
            let time_string = utc.with_timezone(&Local).format("%e %B %k:%M %:z");

            let appendage = format!("[{}] {} -> {}\n", time_string, giver, sender);
            output += &appendage;
        } else {
            panic!("received invalid datatype from redis")
        }
    }

    context
        .send_message_silent(chatid, output)
        .await
        .map_err(|e| format!("sending disaster point count: {:?}", e))?;

    Ok(())
}
