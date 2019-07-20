use crate::db::SqlPool;
use crate::include_sql;
use crate::redis::RedisPool;
use crate::telegram::{
    chat::{Chat, ChatType},
    message::Message,
    Telegram,
};
use crate::util::{get_user, get_user_id, parse_time, rgba_to_cairo};
use cairo::Format;
use chrono::{prelude::*, NaiveDateTime, Utc};
use libc::c_int;
use markov::Chain;

const TIME_FORMAT: &str = "%A, %e %B %Y %H:%M:%S %Z";

//Resolves commands written like /command@foobot which telegram does automatically. Cannot support '@' in command names.
fn get_command<'a>(input: &'a str, botname: &str) -> Option<&'a str> {
    if let Some(at) = input.find('@') {
        if &input[at..] == botname {
            Some(&input[..at])
        } else {
            None
        }
    } else {
        None
    }
}

async fn leaderboards<'a>(
    chatid: i64,
    context: Telegram,
    redis_pool: RedisPool,
    db: SqlPool,
) -> Result<(), String> {
    let conn = db.get().await;
    let messages = conn
        .prepare_cached(include_sql!("getmessages.sql"))
        .unwrap()
        .query_map(params![chatid], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<Vec<(isize, isize)>, rusqlite::Error>>()
        .map_err(|e| format!("getting messages: {:?}", e))?;

    if messages.len() == 0 {
        return context
            .send_message_silent(chatid, "Error: No logged messages in this chat".into())
            .await
            .map(|_| ())
            .map_err(|e| format!("sending no messages exist message: {:?}", e));
    }

    let (total_msgs, since): (isize, isize) = conn
        .query_row(
            include_sql!("getmessagesdata.sql"),
            params![chatid],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|e| format!("getting message data: {:?}", e))?;

    let edits = conn
        .prepare_cached(include_sql!("geteditpercentage.sql"))
        .unwrap()
        .query_map(params![chatid], |row| {
            let user_id = row.get(0)?;
            let edit_percentage = row.get(1)?;
            let total_edits = row.get(2)?;
            Ok((user_id, edit_percentage, total_edits))
        })
        .unwrap()
        .collect::<Result<Vec<(isize, f64, isize)>, rusqlite::Error>>()
        .map_err(|e| format!("getting edit percentage: {:?}", e))?;

    let since = chrono::Local.timestamp(since as i64, 0);
    let redis = redis_pool.get().await;

    //Message counts
    let mut reply = format!("{} messages since {}\n", total_msgs, since);
    let mut messages = messages.into_iter();
    if let Some((user, count)) = messages.next() {
        //store appendage here because otherwise this future doesn't implement Sync for
        //whatever reason
        let appendage = format!(
            "{} is the most annoying having sent {} messages!\n",
            get_user(chatid, user as i64, context.clone(), redis.clone()).await,
            count
        );

        reply += &appendage;
    }

    for m in messages {
        let appendage = format!(
            "{}: {} messages\n",
            get_user(chatid, m.0 as i64, context.clone(), redis.clone()).await,
            m.1
        );
        reply += &appendage;
    }

    // Edits
    let mut edits = edits.into_iter();
    if let Some((user, percentage, count)) = edits.next() {
        let appendage = format!(
            "{} is the biggest disaster, having edited {:.2}% of their messages({} edits total)!\n",
            get_user(chatid, user as i64, context.clone(), redis.clone()).await,
            percentage,
            count
        );
        reply += &appendage;
    }
    for (user, percentage, count) in edits {
        let appendage = format!(
            "{}: {:.2}% ({})\n",
            get_user(chatid, user as i64, context.clone(), redis.clone()).await,
            percentage,
            count
        );

        reply += &appendage;
    }
    context
        .send_message_silent(chatid, reply)
        .await
        .map(|_| ())
        .map_err(|e| format!("sending message: {:?}", e))
}

pub async fn stickerlog<'a>(
    msg: &'a Message,
    args: &'a [String],
    context: Telegram,
    redis_pool: RedisPool,
    db: SqlPool,
) -> Result<(), String> {
    let (caption, images, usages) = {
        let parsed_time = parse_time(&args[1..]);
        if args.len() > 1 && parsed_time.is_none() {
            context
                .send_message_silent(msg.chat.id, "Invalid time string".to_string())
                .await
                .map_err(|e| format!("sending error message: {:?}", e))?;
            return Ok(());
        }
        let from_time: DateTime<Utc> = match parsed_time {
            Some(t) => Utc::now() - t,
            None => DateTime::<Utc>::from_utc(NaiveDateTime::from_timestamp(0, 0), Utc),
        };

        let conn = db.get().await;
        //Build caption message
        let res = conn
            .query_row(
                include_sql!("getstickerstats.sql"),
                params![msg.chat.id as isize, from_time.timestamp() as isize],
                |row| {
                    let total_stickers = row.get(0)?;
                    let packs = row.get(1)?;
                    Ok((total_stickers, packs))
                },
            )
            .map_err(|e| format!("getting sticker stats: {:?}", e))?;

        //For some reason type inferrance breaks when trying to assign these directly
        let (total_stickers, packs): (isize, isize) = res;

        if total_stickers == 0 {
            context
                .send_message_silent(
                    msg.chat.id,
                    format!(
                        "I have no recorded stickers after {}",
                        from_time.with_timezone(&Local).format(TIME_FORMAT)
                    ),
                )
                .await
                .map_err(|e| format!("sending error message: {:?}", e))?;

            return Ok(());
        }

        let caption = format!(
            "{} sent stickers from {} packs since {}",
            total_stickers,
            packs,
            if from_time.naive_utc().timestamp() == 0 {
                "the dawn of time".to_string()
            } else {
                format!("{}", from_time.with_timezone(&Local).format(TIME_FORMAT))
            }
        );

        //Image rendering data
        let logs = conn
            .prepare_cached(include_sql!("getstickercounts.sql"))
            .unwrap()
            .query_map(
                params![msg.chat.id as isize, from_time.timestamp() as isize],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap()
            .collect::<Result<Vec<(String, i32)>, rusqlite::Error>>()
            .map_err(|e| format!("getting sticker counts: {:?}", e))?;

        let (file_ids, usages): (Vec<String>, Vec<i32>) = logs.into_iter().unzip();
        //Get sticker images
        let mut redis = redis_pool.get().await;
        let mut images = Vec::new();
        for f in file_ids {
            let key = format!("tg.download.{}", f);
            match redis.get_bytes(&key).await {
                Ok(Some(v)) => images.push(v),
                Ok(None) => {
                    debug!("File {} not saved in redis, downloading from Telegram", f);
                    let file_key = context
                        .download_file(redis.clone(), &f)
                        .await
                        .map_err(|e| format!("downloading file {}: {:?}", f, e))?;
                    images.push(
                        redis
                            .get_bytes(&file_key)
                            .await
                            .map_err(|e| format!("getting image from Redis: {}: {:?}", f, e))?
                            .unwrap(),
                    );
                }
                Err(e) => return Err(format!("communicating with Redis: {:?}", e)),
            }
        }
        (caption, images, usages)
    };

    //Actual image rendering
    let mut rendered_image = Vec::new();
    {
        //Fun constants to play with
        let height = 1200;
        let padding = 50.0; //padding between bars
        let bar_thickness = 40.0;
        let sticker_thickness = 200.0; // Target sticker thickness
        let max_height = 200.0; //Maximum sticker height
                                //Offset by a bit to prevent the text from clipping at the right edge
        let width = usages.len() as i32 * (padding as i32 + sticker_thickness as i32) + 100;

        let y_scale = (f64::from(height) - max_height - (padding * 2.0))
            / f64::from(*usages.iter().max_by(|x, y| x.cmp(&y)).unwrap());

        let surface = cairo::ImageSurface::create(Format::ARgb32, width, height).unwrap();
        let cairo = cairo::Context::new(&surface);
        cairo.scale(1.0, 1.0);

        #[allow(clippy::unnecessary_cast)]
        cairo.set_source_rgba(
            0x2E as f64 / 0xFF as f64,
            0x2E as f64 / 0xFF as f64,
            0x2E as f64 / 0xFF as f64,
            1.0,
        );
        cairo.rectangle(0.0, 0.0, f64::from(width), f64::from(height));
        cairo.fill();

        for (index, num) in usages.iter().enumerate() {
            unsafe {
                //Stickers are always webp
                let current_image = &images[index];
                let mut image_width: c_int = 0;
                let mut image_height: c_int = 0;
                let image = libwebp_sys::WebPDecodeRGBA(
                    current_image.as_ptr(),
                    current_image.len(),
                    &mut image_width as *mut c_int,
                    &mut image_height as *mut c_int,
                );

                if image.is_null() {
                    return Err("decoding webp image".into());
                }

                let len = image_width as usize * image_height as usize * 4;
                rgba_to_cairo(image, len); //Cairo is bgra on little-endian machines
                let slice = std::slice::from_raw_parts_mut(image, len);
                let format = Format::ARgb32;
                let stride = format.stride_for_width(image_width as u32).unwrap();
                let surface = cairo::ImageSurface::create_for_data(
                    slice,
                    format,
                    image_width,
                    image_height,
                    stride,
                )
                .unwrap();

                let x_offset = index as f64 * (sticker_thickness + padding);
                //Decrease the sticker size until they match the target thickness or maximum height
                let scale_factor = (sticker_thickness / f64::from(image_width))
                    .min(max_height / f64::from(image_height));

                //Sticker image itself
                cairo.scale(scale_factor, scale_factor);
                cairo.set_source_surface(&surface, x_offset * (1.0 / scale_factor), 0.0);
                cairo.paint();
                surface.finish();
                libwebp_sys::WebPFree(image as *mut std::ffi::c_void);

                //Bar graphs
                let normalized_x_offset = x_offset + 0.5 * f64::from(image_width) * scale_factor; //Middle of sticker
                cairo.scale(1.0 / scale_factor, 1.0 / scale_factor); //undo scaling
                cairo.rectangle(
                    normalized_x_offset,
                    max_height + padding,
                    bar_thickness,
                    f64::from(*num) * y_scale,
                );
                cairo.set_source_rgb(0.5, 0.5, 1.0);
                cairo.fill();

                //Usage text
                let num_text = format!("{} uses", num);
                cairo.select_font_face("Hack", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
                cairo.set_font_size(40.0);

                let extents = cairo.text_extents(&num_text);
                cairo.move_to(
                    normalized_x_offset + extents.width / 2.0 - extents.x_bearing,
                    max_height + padding + (f64::from(*num) * y_scale) + extents.height / 2.0
                        - extents.y_bearing,
                );
                cairo.set_source_rgba(1.0, 1.0, 1.0, 1.0);
                cairo.show_text(&num_text);
            }
        }

        surface.write_to_png(&mut rendered_image).unwrap();
    }
    context
        .send_photo(msg.chat.id, rendered_image, Some(caption), true)
        .await
        .map(|_| ())
        .map_err(|e| format!("sending image: {:?}", e))
}

struct MessageData {
    message: String,
    userid: i64,
    instant: i64,
}

//How long until a message gets timed out
const MESSAGE_COMBINE_TIMEOUT: i64 = 30;

//Take as many messages as possible and merge them to create a more cohesive message
//resulting in much better training data for the markov chains
fn merge_messages(messages: &[MessageData], mut index: usize) -> (usize, String) {
    let mut output = messages[index].message.clone();
    while index + 1 < messages.len()
        && messages[index].userid == messages[index + 1].userid
        && (messages[index + 1].instant - messages[index].instant) < MESSAGE_COMBINE_TIMEOUT
    {
        index += 1;
        output += &format!(" {}", messages[index].message); //add a space inbetween each
    }

    (index, output)
}

//This function is almost identical to simulate except it creates a chain with messages from every user and not just one
async fn simulate_chat(
    chat: Chat,
    context: Telegram,
    db_pool: SqlPool,
    redis_pool: RedisPool,
) -> Result<(), String> {
    let key = format!("tg.markovchain.chat.{}", chat.id);
    let mut redis = redis_pool.get().await;
    let chain = match redis.get_bytes(&key).await {
        Ok(Some(s)) => {
            //A cached version exists, use that
            let value: Chain<String> = rmp_serde::from_slice(&s)
                .map_err(|e| format!("deserializing markov chain at {}: {}", key, e))?;
            Ok(value)
        }
        Ok(None) => {
            //Create a new chain
            let mut chain = Chain::new();
            let messages = db_pool
                .get()
                .await
                .prepare_cached(include_sql!("getmessagetext.sql"))
                .unwrap()
                .query_map(params![chat.id], |row| {
                    Ok(MessageData {
                        message: row.get(0)?,
                        userid: row.get(1)?,
                        instant: row.get(2)?,
                    })
                })
                .unwrap()
                .collect::<Result<Vec<MessageData>, rusqlite::Error>>()
                .map_err(|e| format!("getting user message text: {:?}", e))?;

            if messages.len() == 0 {
                return context
                    .send_message_silent(chat.id, "Error: No logged messages in this chat".into())
                    .await
                    .map(|_| ())
                    .map_err(|e| format!("sending no messages exist message: {:?}", e));
            }

            let mut i = 0;
            while i < messages.len() {
                let (index, merged) = merge_messages(&messages, i);
                chain.feed_str(&merged);
                i = index + 1;
            }
            //Cache for later
            let serialized = rmp_serde::to_vec(&chain).unwrap();
            redis
                .set_with_expiry(&key, serialized, std::time::Duration::new(3600, 0))
                .await
                .unwrap();
            Ok(chain)
        }
        Err(e) => Err(format!("redis failure getting markov chain data: {}", e)),
    }?;

    let mut output = format!("Simulated {}: {}", chat, chain.generate_str());

    //Telegram limits message size
    output.truncate(4000);

    context
        .send_message_silent(chat.id, output)
        .await
        .map(|_| ())
        .map_err(|e| format!("sending simulated string: {:?}", e))
}

pub async fn simulate(
    userid: i64,
    chatid: i64,
    context: Telegram,
    db_pool: SqlPool,
    redis_pool: RedisPool,
) -> Result<(), String> {
    let key = format!("tg.markovchain.{}.{}", chatid, userid);
    let mut redis = redis_pool.get().await;
    let chain = match redis.get_bytes(&key).await {
        Ok(Some(s)) => {
            //A cached version exists, use that
            let value: Chain<String> = rmp_serde::from_slice(&s)
                .map_err(|e| format!("deserializing markov chain at {}: {}", key, e))?;
            Ok(value)
        }
        Ok(None) => {
            //Create a new chain
            let mut chain = Chain::new();
            let messages = db_pool
                .get()
                .await
                .prepare_cached(include_sql!("getmessagetext.sql"))
                .unwrap()
                .query_map(params![chatid], |row| {
                    Ok(MessageData {
                        message: row.get(0)?,
                        userid: row.get(1)?,
                        instant: row.get(2)?,
                    })
                })
                .unwrap()
                .collect::<Result<Vec<MessageData>, rusqlite::Error>>()
                .map_err(|e| format!("getting user message text: {:?}", e))?;

            if messages.len() == 0 {
                return context
                    .send_message_silent(chatid, "Error: No logged messages in this chat".into())
                    .await
                    .map(|_| ())
                    .map_err(|e| format!("sending no messages exist message: {:?}", e));
            }

            let mut i = 0;
            while i < messages.len() {
                if messages[i].userid == userid {
                    let (index, merged) = merge_messages(&messages, i);
                    chain.feed_str(&merged);
                    i = index + 1;
                } else {
                    i += 1;
                }
            }
            //Cache for later
            let serialized = rmp_serde::to_vec(&chain).unwrap();
            redis
                .set_with_expiry(&key, serialized, std::time::Duration::new(3600, 0))
                .await
                .unwrap();
            Ok(chain)
        }
        Err(e) => Err(format!("redis failure getting markov chain data: {}", e)),
    }?;

    let mut output = format!(
        "{}<s>: {}",
        get_user(chatid, userid, context.clone(), redis.clone()).await,
        chain.generate_str()
    );

    //Telegram limits message size
    output.truncate(4000);

    context
        .send_message_silent(chatid, output)
        .await
        .map(|_| ())
        .map_err(|e| format!("sending simulated string: {:?}", e))
}

pub async fn quote(
    userid: i64,
    chatid: i64,
    context: Telegram,
    db_pool: SqlPool,
    redis_pool: RedisPool,
) -> Result<(), String> {
    let (message, timestamp): (String, isize) = db_pool
        .get()
        .await
        .query_row(
            include_sql!("getrandomusermessage.sql"),
            params![chatid, userid],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|e| format!("getting random quote: {:?}", e))?;

    let date: DateTime<Local> = Utc.timestamp(timestamp as i64, 0).with_timezone(&Local);

    let redis = redis_pool.get().await;

    context
        .send_message_silent(
            chatid,
            format!(
                "\"{}\" -- {}, {}",
                message,
                get_user(chatid, userid, context.clone(), redis.clone()).await,
                date
            ),
        )
        .await
        .map(|_| ())
        .map_err(|e| format!("sending qoute: {:?}", e))
}

pub async fn handle_command<'a>(
    msg: &'a Message,
    msg_text: &'a str,
    context: Telegram,
    redis_pool: RedisPool,
    db_pool: SqlPool,
) {
    let split: Vec<String> = msg_text.split_whitespace().map(|s| s.into()).collect();
    let root = if let ChatType::Private = msg.chat.kind {
        split[0].as_str()
    } else {
        let command = get_command(&split[0], context.bot_mention());
        if command.is_none() {
            return;
        }
        command.unwrap()
    };

    //Macro for extracting user name
    macro_rules! with_user {
        ($fun:ident ( _, $( $arg:expr ),* ) ) => {
            if split.len() == 2 {
                match get_user_id(msg.chat.id, &split[1], db_pool.clone()).await {
                    Some(u) => {
                        $fun(u, $($arg),*).await
                    }
                    None => context
                        .send_message_silent(
                            msg.chat.id,
                            format!("I haven't seen {} yet", &split[1]),
                        )
                        .await
                        .map(|_| ())
                        .map_err(|e| format!("sending invalid user message: {:?}", e)),
                }
            } else {
                let users = db_pool.get().await
                    .prepare_cached(include_sql!("getchatusers.sql"))
                    .unwrap()
                    .query_map(params![msg.chat.id], |row| Ok(row.get(0)?))
                    .unwrap()
                    .collect::<Result<Vec<String>, rusqlite::Error>>()
                    .map_err(|e| error!("Couldn't get users: {:?}", e));

                if users.is_err() {
                    return;
                }

                let mut users_iter = users.unwrap().into_iter();
                let mut buttons = Vec::new();

                while let Some(u) = users_iter.next() {
                    let mut row = Vec::with_capacity(3);
                    row.push(serde_json::json!({"text": u}));
                    let mut offset = 1;
                    while let Some(u) = users_iter.next() {
                        row.push(serde_json::json!({"text": u}));
                        offset += 1;
                        if offset >= 3 { //&& is not allowed in `while let` so do this
                            break;
                        }
                    }
                    buttons.push(row);
                }

                let reply_message = context
                    .reply_with_markup(
                        msg.id,
                        msg.chat.id,
                        format!("Please select a user to {}", stringify!($fun)),
                        serde_json::json!({
                            "keyboard": buttons,
                            "one_time_keyboard": true,
                            "selective": true,
                        })
                    )
                    .await
                    .map_err(|e| format!("sending error message: {:?}", e)).unwrap();

                let mut redis = redis_pool.get().await;

                let key = format!("tg.replycommand.{}.{}", msg.chat.id, reply_message.id);
                redis
                    .set_with_expiry(&key, stringify!($fun), std::time::Duration::new(3600, 0))
                    .await
                    .unwrap();

                Ok(())
            }
        };
    }
    //Potential improvement: ignore handling commands in private chats since they are explicitly not logged anyway
    let res = match root {
        "/leaderboards" => leaderboards(msg.chat.id, context.clone(), redis_pool, db_pool).await,
        "/stickerlog" => stickerlog(msg, &split, context.clone(), redis_pool, db_pool).await,
        "/quote" => with_user!(quote(_, msg.chat.id, context.clone(), db_pool, redis_pool)),
        "/simulate" => with_user!(simulate(_, msg.chat.id, context.clone(), db_pool, redis_pool)),
        "/simulatechat" => {
            simulate_chat(msg.chat.clone(), context.clone(), db_pool, redis_pool).await
        }
        _ => {
            if let ChatType::Private = msg.chat.kind {
                //Only nag at the user for wrong command if in a private chat
                context
                    .send_message_silent(msg.chat.id, "No such command".to_string())
                    .await
                    .map(|_| ())
                    .map_err(|e| format!("sending no such command message: {:?}", e))
            } else {
                Ok(())
            }
        }
    };

    if let Err(e) = res {
        error!("Command '{}' failed at '{}'", &root[1..], e);
        context
            .send_message_silent(
                msg.chat.id,
                "Fatal error occurred in command, see bot log".into(),
            )
            .await
            .unwrap();
    }
}
