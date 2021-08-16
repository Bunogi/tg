use crate::{
    include_sql, params,
    telegram::{message::Message, Telegram},
    util::{parse_time, rgba_to_cairo},
    Context,
};
use cairo::Format;
use chrono::{prelude::*, NaiveDateTime, Utc};
use darkredis::Connection;
use image::ImageDecoder;
use libc::c_int;
use tokio::{stream::StreamExt, task};
use tokio_postgres::Client;

//Convert the .tgs file into a webp and return the bytes.
async fn get_converted_tgs(
    context: &Context,
    redis: &mut Connection,
    file_id: &str,
) -> Result<Vec<u8>, String> {
    //Has it already been converted?
    let converted_key = format!("tg.conv-animated-sticker.{}", file_id);
    if let Some(v) = redis
        .get(&converted_key)
        .await
        .map_err(|e| format!("getting converted animated sticker from Redis: {}", e))?
    {
        Ok(v)
    } else {
        //No, convert it
        info!("Converting {} into webp...", file_id);

        let temp_convert_key = format!("tg.tempconvert.{}", file_id);

        //Execute tgs->png converter script
        let child = tokio::process::Command::new("./convert.py")
            .arg(&context.config.redis.address)
            .arg(format!("tg.download.{}", file_id))
            .arg(&temp_convert_key)
            .spawn();
        let exit_status = child
            .map_err(|e| format!("Failed to spawn child: {}", e))?
            .await
            .map_err(|e| format!("Convert command failed to run: {}", e))?;

        if exit_status.success() {
            debug!("Converted .tgs into PNG, now converting to webp...");
            let png_data = redis
                .get(&temp_convert_key)
                .await
                .map_err(|e| format!("getting converted png from Redis: {}", e))?
                .expect("getting png data from key");

            redis
                .del(temp_convert_key)
                .await
                .expect("deleting temporary PNG key");

            //Decode PNG into bytes
            let decoder = image::png::PngDecoder::new(png_data.as_slice()).unwrap();
            if decoder.color_type() != image::ColorType::Rgba8 {
                return Err(format!("Expected Rgba8, got {:?}", decoder.color_type()));
            }
            let (width, height) = decoder.dimensions();
            let mut buf = vec![0; decoder.total_bytes() as usize];
            decoder.read_image(&mut buf).unwrap();

            let out = unsafe {
                //Unsafe for FFI with libwebp
                let (width, height): (c_int, c_int) = (width as i32, height as i32);
                let mut output: *mut u8 = std::ptr::null_mut();
                let byte_size = libwebp_sys::WebPEncodeLosslessRGBA(
                    buf.as_ptr(),
                    width,
                    height,
                    width * 4,
                    &mut output as *mut *mut u8,
                );
                if byte_size == 0 {
                    return Err("Failed to encode WebP".into());
                }

                //I'm not sure there's a better way to do this than just copying the memory to make it safe
                let mut out = vec![0u8; byte_size];
                std::ptr::copy_nonoverlapping(output, out.as_mut_ptr(), byte_size);
                libwebp_sys::WebPFree(output as *mut std::ffi::c_void);
                out
            };
            //Store for later
            redis
                .set(converted_key, &out)
                .await
                .expect("setting converted webp");
            Ok(out)
        } else {
            Err("convert command failed".into())
        }
    }
}

struct StickerInfo {
    file_id: String,
    data: Vec<u8>,
}

pub async fn stickerlog<'a>(
    msg: &'a Message,
    args: &'a [String],
    telegram: &Telegram,
    context: &Context,
) -> Result<(), String> {
    let (caption, images, usages, file_ids) = {
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

        let conn = context.db_pool.get().await.unwrap();
        //Build caption message
        let (total_stickers, packs): (i64, i64) = conn
            .query_one(
                include_sql!("getstickerstats.sql"),
                params![msg.chat.id, from_time.timestamp()],
            )
            .await
            .map(|row| (row.get(0), row.get(1)))
            .map_err(|e| format!("getting sticker stats: {:?}", e))?;

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
        let (hashes, usages): (Vec<Vec<u8>>, Vec<i64>) = conn
            .query(
                include_sql!("getstickercounts.sql"),
                params![msg.chat.id, from_time.timestamp()],
            )
            .await
            .map_err(|e| format!("getting sticker counts: {:?}", e))?
            .into_iter()
            .map(|row| (row.get(0), row.get::<usize, i64>(1)))
            .unzip();

        let statement = conn
            .prepare(include_sql!("getstickerfromhash.sql"))
            .await
            .unwrap();

        let mut file_ids: Vec<String> = Vec::new();
        for hash in hashes {
            let id = conn
                .query_one(&statement, &[&hash])
                .await
                .map(|row| row.get(0))
                .map_err(|e| format!("getting hash: {}", e))?;
            file_ids.push(id);
        }

        //Get sticker images
        let mut redis = context.redis_pool.get().await;
        let mut images = Vec::new();
        for f in &file_ids {
            let image = telegram
                .download_file(&mut redis, &f)
                .await
                .map_err(|e| format!("downloading file {}: {}", f, e))?;

            //Check for webp/riff header
            if &image[0..4] != b"RIFF" {
                debug!("Have to convert animated sticker");
                let image = get_converted_tgs(context, &mut redis, f).await?;
                images.push(image);
            } else {
                //Image is already a webp, no need to convert
                images.push(image);
            }
        }
        (caption, images, usages, file_ids)
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
            / f64::from(*usages.iter().max_by(|x, y| x.cmp(&y)).unwrap() as i32);

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
                    return Err(format!("decoding image {} as webp", file_ids[index]));
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
                    f64::from(*num as i32) * y_scale,
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
                    max_height
                        + padding
                        + (f64::from(*num as i32) * y_scale)
                        + extents.height / 2.0
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
