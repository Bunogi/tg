use crate::{
    include_sql, params,
    telegram::{message::Message, Telegram},
    util::{parse_time, rgba_to_cairo},
    Context,
};
use cairo::Format;
use chrono::{prelude::*, NaiveDateTime, Utc};
use libc::c_int;
use tokio::task;

struct Image {
    data: *mut u8,
    size: usize,
    width: i32,
    height: i32,
}

fn decode_webp(input: &[u8]) -> Result<Image, String> {
    let mut width: c_int = 0;
    let mut height: c_int = 0;
    unsafe {
        let data = libwebp_sys::WebPDecodeRGBA(
            input.as_ptr(),
            input.len(),
            &mut width as *mut c_int,
            &mut height as *mut c_int,
        );

        if data.is_null() {
            return Err("decoding image as webp".to_string());
        }
        let size = width as usize * height as usize * 4; // RGBA
        Ok(Image {
            data,
            size,
            width,
            height,
        })
    }
}

fn render_image(stickers_webp: Vec<Vec<u8>>, usages: Vec<i64>) -> Result<Vec<u8>, String> {
    let decoded = stickers_webp
        .into_iter()
        .map(|s| decode_webp(&s))
        .collect::<Result<Vec<Image>, String>>()?;
    unsafe {
        decoded
            .iter()
            .for_each(|image| rgba_to_cairo(image.data, image.size));
    }

    let retval = render_image_inner(&decoded, usages);

    decoded.iter().for_each(|i| unsafe {
        libwebp_sys::WebPFree(i.data as *mut std::ffi::c_void);
    });

    retval.map_err(|e| format!("Cairo error {:?}\n", e))
}

fn render_image_inner(stickers: &[Image], usages: Vec<i64>) -> Result<Vec<u8>, cairo::Error> {
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
        / f64::from(*usages.iter().max_by(|x, y| x.cmp(y)).unwrap() as i32);

    let surface = cairo::ImageSurface::create(Format::ARgb32, width, height)?;
    let cairo = cairo::Context::new(&surface)?;
    cairo.scale(1.0, 1.0);

    #[allow(clippy::unnecessary_cast)]
    cairo.set_source_rgba(
        0x2E as f64 / 0xFF as f64,
        0x2E as f64 / 0xFF as f64,
        0x2E as f64 / 0xFF as f64,
        1.0,
    );
    cairo.rectangle(0.0, 0.0, f64::from(width), f64::from(height));
    cairo.fill()?;

    for (index, num) in usages.iter().enumerate() {
        let image = &stickers[index];
        let slice = unsafe { std::slice::from_raw_parts_mut(image.data, image.size) };

        let format = Format::ARgb32;
        let stride = format.stride_for_width(image.width as u32)?;
        let surface =
            cairo::ImageSurface::create_for_data(slice, format, image.width, image.height, stride)?;

        let x_offset = index as f64 * (sticker_thickness + padding);
        //Decrease the sticker size until they match the target thickness or maximum height
        let scale_factor =
            (sticker_thickness / f64::from(image.width)).min(max_height / f64::from(image.height));

        //Sticker image itself
        cairo.scale(scale_factor, scale_factor);
        cairo.set_source_surface(&surface, x_offset * (1.0 / scale_factor), 0.0)?;
        cairo.paint()?;
        surface.finish();

        //Bar graphs
        let normalized_x_offset = x_offset + 0.5 * f64::from(image.height) * scale_factor; //Middle of sticker
        cairo.scale(1.0 / scale_factor, 1.0 / scale_factor); //undo scaling
        cairo.rectangle(
            normalized_x_offset,
            max_height + padding,
            bar_thickness,
            f64::from(*num as i32) * y_scale,
        );
        cairo.set_source_rgb(0.5, 0.5, 1.0);
        cairo.fill()?;

        //Usage text
        let num_text = format!("{} uses", num);
        cairo.select_font_face("Hack", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
        cairo.set_font_size(40.0);

        let extents = cairo.text_extents(&num_text)?;
        cairo.move_to(
            normalized_x_offset + extents.width() / 2.0 - extents.x_bearing(),
            max_height + padding + (f64::from(*num as i32) * y_scale) + extents.height() / 2.0
                - extents.y_bearing(),
        );
        cairo.set_source_rgba(1.0, 1.0, 1.0, 1.0);
        cairo.show_text(&num_text)?;
    }

    //FIXME: get rid of this expect?
    surface
        .write_to_png(&mut rendered_image)
        .expect("failed to write rendersurface to PNG");
    Ok(rendered_image)
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
            None => {
                DateTime::<Utc>::from_utc(NaiveDateTime::from_timestamp_opt(0, 0).unwrap(), Utc)
            }
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
        let (hashes, mut usages): (Vec<Vec<u8>>, Vec<i64>) = conn
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

        let mut redis = context.redis_pool.get().await;
        let mut images = Vec::new();
        for (index, hash) in hashes.iter().enumerate() {
            let id = conn
                .query_one(&statement, &[&hash])
                .await
                .map(|row| row.get::<usize, String>(0))
                .map_err(|e| format!("getting sticker file id from hash: {}", e))?;

            let image = telegram
                .download_file(&mut redis, &id)
                .await
                .map_err(|e| format!("downloading file {}: {}", id, e))?;

            if &image[0..4] != b"RIFF" {
                info!("Sticker {} is not a webp, ignoring", id);
                usages.remove(index);
            } else {
                images.push(image);
            }
        }

        (caption, images, usages)
    };

    //Actual image rendering
    let rendered_image = task::block_in_place(|| render_image(images, usages))?;

    telegram
        .send_png_lossless(msg.chat.id, rendered_image, Some(caption), true)
        .await
        .map(|_| ())
        .map_err(|e| format!("sending image: {}", e))
}
