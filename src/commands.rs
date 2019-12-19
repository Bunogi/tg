use crate::{
    include_sql,
    telegram::{
        chat::{Chat, ChatType},
        message::Message,
        Telegram,
    },
    util::{align_text_after, get_user, get_user_id, parse_time, rgba_to_cairo},
    Context,
};
use cairo::Format;
use chrono::{prelude::*, NaiveDateTime, Utc};
use libc::c_int;
use markov::Chain;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use tokio::task;
use unicode_segmentation::UnicodeSegmentation;

pub mod disaster;

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
    telegram: &Telegram,
    context: &Context,
) -> Result<(), String> {
    let conn = context.db_pool.get().await;
    let messages = task::block_in_place(|| {
        conn.prepare_cached(include_sql!("getmessages.sql"))
            .unwrap()
            .query_map(params![chatid], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<Result<Vec<(isize, isize)>, rusqlite::Error>>()
            .map_err(|e| format!("getting messages: {:?}", e))
    })?;

    if messages.is_empty() {
        return telegram
            .send_message_silent(chatid, "Error: No logged messages in this chat".into())
            .await
            .map(|_| ())
            .map_err(|e| format!("sending no messages exist message: {}", e));
    }

    let (total_msgs, since): (isize, isize) = task::block_in_place(|| {
        conn.query_row(
            include_sql!("getmessagesdata.sql"),
            params![chatid],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|e| format!("getting message data: {:?}", e))
    })?;

    let since = chrono::Local.timestamp(since as i64, 0);
    let mut redis = context.redis_pool.get().await;

    //Message counts
    let mut reply = format!("```\n{} messages since {}\n", total_msgs, since);
    let mut messages = messages.into_iter();
    if let Some((user, count)) = messages.next() {
        //store appendage here because otherwise this future doesn't implement Sync for
        //whatever reason
        let appendage = format!(
            "{} is the most annoying having sent {} messages!\n",
            get_user(chatid, user as i64, telegram, &context.config, &mut redis).await,
            count
        );

        reply += &appendage;
    }

    //Stores text to be aligned later
    let mut table = String::new();
    for m in messages {
        let appendage = format!(
            "{}: {} messages\n",
            get_user(chatid, m.0 as i64, telegram, &context.config, &mut redis).await,
            m.1
        );
        table += &appendage;
    }
    reply += &align_text_after(':', table);
    reply += "\n";

    // Edits
    let edits = task::block_in_place(move || {
        conn.prepare_cached(include_sql!("geteditpercentage.sql"))
            .unwrap()
            .query_map(params![chatid], |row| {
                let user_id = row.get(0)?;
                let edit_percentage = row.get(1)?;
                let total_edits = row.get(2)?;
                Ok((user_id, edit_percentage, total_edits))
            })
            .unwrap()
            .collect::<Result<Vec<(isize, f64, isize)>, rusqlite::Error>>()
            .map_err(|e| format!("getting edit percentage: {:?}", e))
    })?;

    let mut edits = edits.into_iter();
    if let Some((user, percentage, count)) = edits.next() {
        let appendage = format!(
            "{} is the biggest disaster, having edited {:.2}% of their messages({} edits total)!\n",
            get_user(chatid, user as i64, telegram, &context.config, &mut redis).await,
            percentage,
            count
        );
        reply += &appendage;
    }
    let mut table = String::new();
    for (user, percentage, count) in edits {
        let appendage = format!(
            "{}: {:.2}% ({})\n",
            get_user(chatid, user as i64, &telegram, &context.config, &mut redis).await,
            percentage,
            count
        );

        table += &appendage;
    }
    reply += &align_text_after(':', table);
    telegram
        .send_message_silently_with_markdown(chatid, format!("{}```", reply))
        .await
        .map(|_| ())
        .map_err(|e| format!("sending leaderboards message: {}", e))
}

pub async fn stickerlog<'a>(
    msg: &'a Message,
    args: &'a [String],
    telegram: &Telegram,
    context: &Context,
) -> Result<(), String> {
    let (caption, images, usages) = {
        let parsed_time = parse_time(&args[1..]);
        if args.len() > 1 && parsed_time.is_none() {
            telegram
                .send_message_silent(msg.chat.id, "Invalid time string".to_string())
                .await
                .map_err(|e| format!("sending error message: {}", e))?;
            return Ok(());
        }
        let from_time: DateTime<Utc> = match parsed_time {
            Some(t) => Utc::now() - t,
            None => DateTime::<Utc>::from_utc(NaiveDateTime::from_timestamp(0, 0), Utc),
        };

        let conn = context.db_pool.get().await;
        //Build caption message
        let res = task::block_in_place(|| {
            conn.query_row(
                include_sql!("getstickerstats.sql"),
                params![msg.chat.id as isize, from_time.timestamp() as isize],
                |row| {
                    let total_stickers = row.get(0)?;
                    let packs = row.get(1)?;
                    Ok((total_stickers, packs))
                },
            )
            .map_err(|e| format!("getting sticker stats: {:?}", e))
        })?;

        //For some reason type inferrance breaks when trying to assign these directly
        let (total_stickers, packs): (isize, isize) = res;

        if total_stickers == 0 {
            telegram
                .send_message_silent(
                    msg.chat.id,
                    format!(
                        "I have no recorded stickers after {}",
                        from_time
                            .with_timezone(&Local)
                            .format(&context.config.general.time_format)
                    ),
                )
                .await
                .map_err(|e| format!("sending error message: {}", e))?;

            return Ok(());
        }

        let caption = format!(
            "{} sent stickers from {} packs since {}",
            total_stickers,
            packs,
            if from_time.naive_utc().timestamp() == 0 {
                "the dawn of time".to_string()
            } else {
                format!(
                    "{}",
                    from_time
                        .with_timezone(&Local)
                        .format(&context.config.general.time_format)
                )
            }
        );

        //Image rendering data
        let logs = task::block_in_place(|| {
            conn.prepare_cached(include_sql!("getstickercounts.sql"))
                .unwrap()
                .query_map(
                    params![msg.chat.id as isize, from_time.timestamp() as isize],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap()
                .collect::<Result<Vec<(String, i32)>, rusqlite::Error>>()
                .map_err(|e| format!("getting sticker counts: {:?}", e))
        })?;

        let (file_ids, usages): (Vec<String>, Vec<i32>) = logs.into_iter().unzip();
        //Get sticker images
        let mut redis = context.redis_pool.get().await;
        let mut images = Vec::new();
        for f in file_ids {
            let image = telegram
                .download_file(&mut redis, &f)
                .await
                .map_err(|e| format!("downloading file {}: {}", f, e))?;
            images.push(image);
        }
        (caption, images, usages)
    };

    //Actual image rendering
    let rendered_image: Result<Vec<u8>, String> = task::block_in_place(|| {
        let mut rendered_image = Vec::new();

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
        Ok(rendered_image)
    });

    telegram
        .send_png_lossless(msg.chat.id, rendered_image?, Some(caption), true)
        .await
        .map(|_| ())
        .map_err(|e| format!("sending image: {}", e))
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
    order: usize,
    chat: &Chat,
    telegram: &Telegram,
    context: &Context,
) -> Result<(), String> {
    let key = format!("tg.markovchain.chat.{}:{}", chat.id, order);
    let mut redis = context.redis_pool.get().await;
    let chain = match redis.get(&key).await {
        Ok(Some(ref s)) => {
            //A cached version exists, use that
            let value: Chain<String> = rmp_serde::from_slice(s)
                .map_err(|e| format!("deserializing markov chain at {}: {}", key, e))?;
            Ok(value)
        }
        Ok(None) => {
            //Create a new chain
            let mut chain = Chain::of_order(order);
            let conn = context.db_pool.get().await;
            let messages = task::block_in_place(|| {
                conn.prepare_cached(include_sql!("getmessagetext.sql"))
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
                    .map_err(|e| format!("getting user message text: {:?}", e))
            })?;

            if messages.is_empty() {
                return telegram
                    .send_message_silent(chat.id, "Error: No logged messages in this chat".into())
                    .await
                    .map(|_| ())
                    .map_err(|e| format!("sending no messages exist message: {}", e));
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
                .set_and_expire_seconds(&key, serialized, context.config.cache.markov_chain as u32)
                .await
                .unwrap();
            Ok(chain)
        }
        Err(e) => Err(format!("redis failure getting markov chain data: {}", e)),
    }?;

    let mut output = format!("Simulated {}: {}", chat, chain.generate_str());

    //Telegram limits message size
    output.truncate(4000);

    telegram
        .send_message_silent(chat.id, output)
        .await
        .map(|_| ())
        .map_err(|e| format!("sending simulated string: {}", e))
}

pub async fn simulate(
    userid: i64,
    chatid: i64,
    order: usize,
    command_message_id: u64,
    telegram: &Telegram,
    context: &Context,
) -> Result<(), String> {
    let key = format!("tg.markovchain.{}.{}:{}", chatid, userid, order);
    let mut redis = context.redis_pool.get().await;
    let chain = match redis.get(&key).await {
        Ok(Some(s)) => {
            //A cached version exists, use that
            let value: Chain<String> = rmp_serde::from_slice(&s)
                .map_err(|e| format!("deserializing markov chain at {}: {}", key, e))?;
            Ok(value)
        }
        Ok(None) => {
            //Create a new chain
            let mut chain = Chain::of_order(order);
            let conn = context.db_pool.get().await;

            let messages = task::block_in_place(|| {
                conn.prepare_cached(include_sql!("getmessagetext.sql"))
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
                    .map_err(|e| format!("getting user message text: {:?}", e))
            })?;

            if messages.is_empty() {
                return telegram
                    .reply_and_close_keyboard(
                        command_message_id,
                        chatid,
                        "Error: No logged messages in this chat".into(),
                    )
                    .await
                    .map(|_| ())
                    .map_err(|e| format!("sending no messages exist message: {}", e));
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
                .set_and_expire_seconds(&key, &serialized, context.config.cache.markov_chain as u32)
                .await
                .unwrap();
            Ok(chain)
        }
        Err(e) => Err(format!("redis failure getting markov chain data: {}", e)),
    }?;

    let mut output = format!(
        "{}<s>: {}",
        get_user(chatid, userid, telegram, &context.config, &mut redis).await,
        chain.generate_str()
    );

    //Telegram limits message size
    output.truncate(4000);

    telegram
        .reply_and_close_keyboard(command_message_id, chatid, output)
        .await
        .map(|_| ())
        .map_err(|e| format!("sending simulated string: {}", e))
}

pub async fn quote(
    userid: i64,
    chatid: i64,
    command_message_id: u64,
    telegram: &Telegram,
    context: &Context,
) -> Result<(), String> {
    let conn = context.db_pool.get().await;
    let (message, timestamp): (String, isize) = task::block_in_place(|| {
        conn.query_row(
            include_sql!("getrandomusermessage.sql"),
            params![chatid, userid],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|e| format!("getting random quote: {:?}", e))
    })?;

    let date: DateTime<Local> = Utc.timestamp(timestamp as i64, 0).with_timezone(&Local);

    let mut redis = context.redis_pool.get().await;

    telegram
        .reply_and_close_keyboard(
            command_message_id,
            chatid,
            format!(
                "\"{}\" -- {}, {}",
                message,
                get_user(chatid, userid, telegram, &context.config, &mut redis).await,
                date
            ),
        )
        .await
        .map(|_| ())
        .map_err(|e| format!("sending qoute: {}", e))
}

async fn wordcount_graph(
    command_message: &Message,
    telegram: &Telegram,
    context: &Context,
) -> Result<(), String> {
    let conn = context.db_pool.get().await;
    let results = task::block_in_place(move || {
        conn.prepare_cached(include_sql!("getwordcounts.sql"))
            .unwrap()
            .query_map(params![command_message.chat.id, 60], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .unwrap()
            .collect::<Result<Vec<(String, u32)>, rusqlite::Error>>()
            .map_err(|e| format!("getting word counts: {:?}", e))
    })?;

    if results.is_empty() {
        telegram
            .reply_to(
                command_message.id,
                command_message.chat.id,
                "There are no logged messages in this chat!".into(),
            )
            .await
            .map(|_| ())
            .map_err(|e| format!("sending log error message: {}", e))
    } else {
        //Initialize image with some constants
        let padding = 20.0;
        let thickness = 25.0;
        let y_shift = 30.0;
        let height_unit = 800.0 / f64::from(results[0].1); //Limit height of bar to 800 and have each other bar be a representation of that
        let width = (padding + thickness) * results.len() as f64 + 50.0;
        let height = (f64::from(results[0].1) * height_unit + 70.0 + y_shift).ceil();
        let font_size = 15.0;

        //Perform this in a block such that the cairo context gets dropped before anything else.
        //Without this, this future won't be Sync.
        let rendered_image = task::block_in_place(|| {
            let surface =
                cairo::ImageSurface::create(Format::ARgb32, width as i32, height as i32).unwrap();
            let cairo = cairo::Context::new(&surface);
            cairo.scale(1.0, 1.0);

            #[allow(clippy::unnecessary_cast)]
            cairo.set_source_rgba(
                0x2E as f64 / 0xFF as f64,
                0x2E as f64 / 0xFF as f64,
                0x2E as f64 / 0xFF as f64,
                1.0,
            );
            cairo.rectangle(0.0, 0.0, width, height);
            cairo.fill();

            for (index, (word, uses)) in results.into_iter().enumerate() {
                cairo.set_source_rgba(0.5, 0.5, 1.0, 1.0);
                let x_pos = (thickness + padding) * index as f64 + padding;
                let bar_height = f64::from(uses) * height_unit;
                cairo.rectangle(x_pos, padding + y_shift, thickness, bar_height);
                cairo.fill();

                //Render the text
                cairo.select_font_face("Hack", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
                cairo.set_font_size(font_size);
                cairo.set_source_rgba(1.0, 1.0, 1.0, 1.0);

                //Number
                let number_y_position = bar_height + 5.0 + y_shift + padding + font_size;
                let number_text = uses.to_string();
                // let extends = cairo.text_extents(&number_text);
                cairo.move_to(x_pos, number_y_position);
                cairo.show_text(&number_text);

                //Stagger the text in order make the words more readable
                let text_y_position = font_size
                    + if index % 2 == 0 {
                        y_shift
                    } else {
                        y_shift / 2.0
                    };
                cairo.move_to(x_pos, text_y_position);
                cairo.show_text(&word);
            }

            let mut rendered_image = Vec::new();
            surface.write_to_png(&mut rendered_image).unwrap();
            rendered_image
        });
        telegram
            .send_png_lossless(command_message.chat.id, rendered_image, None, true)
            .await
            .map(|_| ())
            .map_err(|_| "sending rendered image".to_string())
    }
}

async fn wordcount(
    word: &str,
    chat: &Chat,
    telegram: &Telegram,
    context: &Context,
) -> Result<(), String> {
    let conn = context.db_pool.get().await;
    let usages: isize = task::block_in_place(move || {
        conn.query_row(
            include_sql!("getwordusage.sql"),
            params![word, chat.id],
            |row| row.get(1),
        )
        .unwrap_or(0)
    });

    telegram
        .send_message_silent(
            chat.id,
            format!("I have seen the word '{}' {} time(s).", word, usages),
        )
        .await
        .map(|_| ())
        .map_err(|e| format!("sending word count message: {}", e))
}

async fn charcount(chatid: i64, telegram: &Telegram, context: &Context) -> Result<(), String> {
    let conn = context.db_pool.get().await;
    let messages: Vec<(i64, String)> = task::block_in_place(|| {
        conn.prepare_cached(include_sql!("getmessagebyuser.sql"))
            .unwrap()
            .query_map(params![chatid], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<Result<Vec<(i64, String)>, rusqlite::Error>>()
            .map_err(|e| format!("getting messages and userid: {:?}", e))
    })?;

    if messages.is_empty() {
        return telegram
            .send_message_silent(
                chatid,
                "There are no messages logged in this chat!".to_string(),
            )
            .await
            .map(|_| ())
            .map_err(|e| format!("sending no logged messages error: {}", e));
    }

    //Userid, char count
    let mut char_counts: HashMap<i64, i64> = HashMap::new();
    for (userid, message) in messages {
        for grapheme in message.graphemes(true) {
            //Ignore whitespace
            if !grapheme.trim().is_empty() {
                let counter = char_counts.entry(userid).or_insert(0);
                *counter += 1;
            }
        }
    }

    let mut sorted: Vec<(i64, i64)> = char_counts.into_iter().collect();
    sorted.sort_unstable_by_key(|x| -x.1); //Sorting by the negative puts the largest numbers first
    let mut sorted = sorted.into_iter();
    let first = sorted.next().unwrap();

    let mut redis = context.redis_pool.get().await;

    macro_rules! get_user_msgcount {
        ($userid:expr) => {
            task::block_in_place(|| {
                conn.query_row(
                    include_sql!("getusermessagecount.sql"),
                    params![chatid, $userid],
                    |row| Ok(row.get(0)?),
                )
                .unwrap()
            })
        };
    }

    let msgcount: i64 = get_user_msgcount!(first.0);

    let mut averages = Vec::new();
    let first_user = get_user(chatid, first.0, telegram, &context.config, &mut redis)
        .await
        .to_string();
    let mut output = format!(
        "```\n{} has flooded the most with {} characters sent in {} messages!\n",
        first_user, first.1, msgcount
    );

    averages.push((first_user, first.1 as f32 / msgcount as f32));

    let mut table = String::new();
    for (userid, chars) in sorted {
        let msgcount: i64 = get_user_msgcount!(userid);
        let user = get_user(chatid, userid, telegram, &context.config, &mut redis)
            .await
            .to_string();
        table += &format!("{}: {} ({})\n", &user, chars, msgcount);
        averages.push((user, chars as f32 / msgcount as f32));
    }
    output += &align_text_after(':', table);

    //comparing b to a will cause the sort to go from high->low
    averages.sort_unstable_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    let mut averages = averages.into_iter();
    let first = averages.next().unwrap();
    output += &format!(
        "\n{} is the most literate, sending an average of {:.2} chars per message!\n",
        first.0, first.1
    );
    let mut table = String::new();
    for (user, avg) in averages {
        table += &format!("{}: {:.2}\n", user, avg);
    }
    output += &align_text_after(':', table); //output of align will end with newline
    output += "```";

    telegram
        .send_message_silently_with_markdown(chatid, output)
        .await
        .map(|_| ())
        .map_err(|e| format!("sending char count message: {}", e))
}

fn get_order(from: Option<&String>, context: &Context) -> Result<usize, String> {
    if let Ok(n) = from
        .unwrap_or(&context.config.markov.chain_order.to_string())
        .parse::<usize>()
    {
        if n == 0 || n > context.config.markov.max_order {
            Err(format!(
                "Order must be greater than 0 and no bigger than {}.",
                context.config.markov.max_order
            ))
        } else {
            Ok(n)
        }
    } else {
        Err("Order must be a non-negative integer".into())
    }
}

//Action constants used in the get_user macro for commands which can take a reply.
//A small optimization could be to split these into a human readable and a redis format by using an enum type or something

#[derive(Serialize, Deserialize)]
pub enum ReplyAction {
    Simulate,
    Quote,
    AddDisasterPoint,
}

impl fmt::Display for ReplyAction {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Simulate => write!(f, "simulate"),
            Self::Quote => write!(f, "quote"),
            Self::AddDisasterPoint => write!(f, "give a disaster point"),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct ReplyCommand {
    pub command_message_id: u64, // Original command message which is missing a user
    pub action: ReplyAction,
}

pub async fn handle_command(msg: &Message, msg_text: &str, telegram: &Telegram, context: &Context) {
    let split: Vec<String> = msg_text.split_whitespace().map(|s| s.into()).collect();
    let root = if let ChatType::Private = msg.chat.kind {
        split[0].as_str()
    } else {
        let command = get_command(&split[0], telegram.bot_mention());
        if command.is_none() {
            return;
        }
        command.unwrap()
    };

    //Macro for extracting user name and asking for a user using a keyboard if none is given
    macro_rules! with_user {
        ($action:expr, $fun:ident ( _, $( $arg:expr ),* ) ) => {
            if split.len() >= 2 {
                match get_user_id(msg.chat.id, &split[1], &context.db_pool).await {
                    Some(u) => {
                        $fun(u, $($arg),*).await
                    }
                    None => telegram
                        .send_message_silent(
                            msg.chat.id,
                            format!("I haven't seen {} yet", &split[1]),
                        )
                        .await
                        .map(|_| ())
                        .map_err(|e| format!("sending invalid user message: {}", e)),
                }
            } else {
                //Build keyboard
                let conn = context.db_pool.get().await;
                let users = task::block_in_place(move || {
                    conn
                        .prepare_cached(include_sql!("getchatusers.sql"))
                        .unwrap()
                        .query_map(params![msg.chat.id], |row| Ok(row.get(0)?))
                        .unwrap()
                        .collect::<Result<Vec<String>, rusqlite::Error>>()
                        .map_err(|e| error!("Couldn't get users: {:?}", e))
                });

                if users.is_err() {
                    return;
                }

                let mut users_iter = users.unwrap().into_iter();
                let mut buttons = Vec::new();

                //Group into rows
                const USERNAME_GROUP_WIDTH: usize = 3;
                while let Some(u) = users_iter.next() {
                    let mut row = Vec::with_capacity(USERNAME_GROUP_WIDTH);
                    row.push(serde_json::json!({"text": u}));

                    let mut offset = 1;
                    while let Some(u) = users_iter.next() {
                        row.push(serde_json::json!({"text": u}));
                        offset += 1;
                        if offset >= USERNAME_GROUP_WIDTH { //&& is not allowed in `while let` so do this
                            break;
                        }
                    }
                    buttons.push(row);
                }

                let request_message = telegram
                    .reply_with_markup(
                        msg.id,
                        msg.chat.id,
                        format!("Please select a user to {}", $action),
                        serde_json::json!({
                            "keyboard": buttons,
                            "selective": true,
                        })
                    )
                    .await
                    .map_err(|e| format!("sending select user message: {}", e)).unwrap();

                let reply_command = ReplyCommand {
                    command_message_id: msg.id,
                    action: $action
                };

                let mut redis = context.redis_pool.get().await;

                let key = format!("tg.replycommand.{}.{}", msg.chat.id, request_message.id);
                redis
                    .set_and_expire_seconds(&key, rmp_serde::to_vec(&reply_command).unwrap(), 3600 * 24)
                    .await
                    .unwrap();

                Ok(())
            }
        };
    }

    //Potential improvement: ignore handling commands in private chats since they are explicitly not logged anyway
    let res = match root {
        "/leaderboards" => leaderboards(msg.chat.id, telegram, context).await,
        "/stickerlog" => stickerlog(msg, &split, telegram, context).await,
        "/quote" => with_user!(ReplyAction::Quote, quote(_, msg.chat.id, msg.id, telegram,context)),
        "/simulate" => match get_order(split.get(2), context) {
            Ok(n) => {
                with_user!(ReplyAction::Simulate, simulate(_, msg.chat.id, n, msg.id, telegram, context))
            }
            Err(e) => telegram
                .send_message_silent(msg.chat.id, e)
                .await
                .map(|_| ())
                .map_err(|e| format!("sending invalid order message: {}", e)),
        },
        "/simulatechat" => match get_order(split.get(1), context) {
            Ok(n) => simulate_chat(n, &msg.chat, telegram, context).await,
            Err(e) => telegram
                .send_message_silent(msg.chat.id, e)
                .await
                .map(|_| ())
                .map_err(|e| format!("sending invalid order message: {}", e)),
        },
        "/charcount" => charcount(msg.chat.id, telegram, context).await,
        "/wordcount" => match split.len() {
            1 => wordcount_graph(&msg, telegram, context).await,
            2 => wordcount(&split[1], &msg.chat, telegram, context).await,
            _ => telegram
                .send_message_silent(msg.chat.id, "Invalid number of arguments".to_string())
                .await
                .map(|_| ())
                .map_err(|e| format!("sending invalid argument message: {}", e)),
        },
        "/disaster" => {
            use disaster::add_point;
            with_user!(
                ReplyAction::AddDisasterPoint,
                add_point(_, msg.from.id, msg.chat.id, msg.id, msg.date, telegram, context)
            )
        }
        "/disasterpoints" => disaster::show_points(msg.chat.id, telegram, context).await,
        _ => {
            warn!("No command found for {}", root);
            if let ChatType::Private = msg.chat.kind {
                //Only nag at the user for wrong command if in a private chat
                telegram
                    .send_message_silent(msg.chat.id, "No such command".to_string())
                    .await
                    .map(|_| ())
                    .map_err(|e| format!("sending no such command message: {}", e))
            } else {
                Ok(())
            }
        }
    };

    if let Err(e) = res {
        error!("Command '{}' failed at '{}'", &root[1..], e);
        //If this causes an error something bad must have happened
        let _ = telegram
            .send_message_silent(
                msg.chat.id,
                "Fatal error occurred in command, see bot log".into(),
            )
            .await;
    }
}
