extern crate ffmpeg_next as ffmpeg;

use std::mem;
use std::sync::atomic::{AtomicBool, AtomicI64};
use std::time::Duration;

use clap::{command, Parser};
use ffmpeg::format::{input, Pixel};
use ffmpeg::media::Type;
use ffmpeg::software::scaling::{Context, Flags};
use ffmpeg::util::frame::video::Video;
use ffmpeg::Discard;
use image::DynamicImage;
use lazy_static::lazy_static;
// use reqwest::{multipart, Body, Client};
use serde::{Deserialize, Deserializer, Serialize};
// use tokio::fs::File;
use tokio::time::sleep;
use tracing_subscriber::layer::SubscriberExt;
// use tokio_util::codec::{BytesCodec, FramedRead};
use std::process::Command;
use std::sync::atomic::Ordering::{Relaxed, Release};
use tracing::{debug, error, info};
use tracing_subscriber::util::SubscriberInitExt;
use image::io::Reader as ImageReader;
use ab_glyph::{FontRef, PxScale};

pub mod config;
pub mod output;

use crate::config::Config;
use crate::output::{post_to_social_media, should_post, should_save_image};
use chrono::Local;


#[derive(Parser)]
#[command(version, about, long_about = None, name = "Norppalive Service", author = "Risto \"Dilaz\" Viitanen")]
struct Args {
    #[arg(short, long, default_value = "config.toml")]
    config: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct DetectionResult {
    #[serde(deserialize_with = "deserialize_box")]
    r#box: [u32; 4],
    #[serde(deserialize_with = "float_to_u8")]
    cls: u8,
    cls_name: String,
    #[serde(deserialize_with = "deserialize_conf")]
    conf: u8,
}

fn float_to_u8<'de, D>(deserializer: D) -> Result<u8, D::Error>
where
    D: Deserializer<'de>,
{
    let f = f32::deserialize(deserializer)?;
    Ok((f * 1.0) as u8)
}

fn deserialize_box<'de, D>(deserializer: D) -> Result<[u32; 4], D::Error>
where
    D: Deserializer<'de>,
{
    let vec: Vec<f32> = Vec::deserialize(deserializer)?;
    Ok([vec[0] as u32, vec[1] as u32, vec[2] as u32, vec[3] as u32])
}

fn deserialize_conf<'de, D>(deserializer: D) -> Result<u8, D::Error>
where
    D: Deserializer<'de>,
{
    let f = f32::deserialize(deserializer)?;
    Ok((f * 100.0) as u8)
}

lazy_static! {
    static ref ARGS: Args = Args::parse();
    static ref SHUTDOWN: AtomicBool = AtomicBool::new(false);
    static ref CONFIG: Config =  toml::from_str(&std::fs::read_to_string(&ARGS.config).unwrap()).unwrap();
    static ref SAVE_IMAGE: AtomicBool = AtomicBool::new(true);
    static ref LAST_POST_TIME: AtomicI64 = AtomicI64::new(0);
    static ref LAST_IMAGE_SAVE_TIME: AtomicI64 = AtomicI64::new(0);
}

#[tokio::main]
async fn main() -> Result<(), String> {
    tracing::info!("Starting!");
    // initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "norppalive_service=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    ffmpeg::init().map_err(|err| format!("{}", err))?;

    // Set log level to suppress FFmpeg logs
    ffmpeg::log::set_level(ffmpeg::log::Level::Quiet);

    let stream_url = get_stream_url(&CONFIG.stream.stream_url).map_err(|err| format!("Could not get stream URL: {}", err))?;
    let mut ictx = input(&stream_url).map_err(|err| format!("{}", err))?;
    let input = ictx
        .streams()
        .best(Type::Video)
        .ok_or("Could not open the stream".to_string())?;

    let video_stream_index = input.index();

    let mut decoder = input.codec().decoder().video().map_err(|err| format!("{}", err))?;
    decoder.skip_frame(Discard::NonKey);

    let mut scaler = Context::get(
        decoder.format(),
        decoder.width(),
        decoder.height(),
        Pixel::RGB24,
        decoder.width(),
        decoder.height(),
        Flags::BILINEAR,
    )
    .map_err(|err| format!("{}", err))?;

    let mut frame_index = 0u64;
    let mut skipped = 0u64;

    let mut receive_and_process_decoded_frames =
    |decoder: &mut ffmpeg::decoder::Video| -> Result<(), String> {
        let mut decoded = Video::empty();
        while decoder.receive_frame(&mut decoded).is_ok() {
            if SAVE_IMAGE.load(Relaxed) {
                info!("Saving image");
                let mut rgb_frame = Video::empty();
                scaler.run(&decoded, &mut rgb_frame).map_err(|err| format!("Could not run scaler: {}", err))?;
                save_file(&rgb_frame, &CONFIG.image_filename)?;
                info!("Saved image {} to {}", frame_index, &CONFIG.image_filename);
                SAVE_IMAGE.store(false, Relaxed);
            } else {
                skipped += 1;
                debug!("Skipped frames: {}", skipped);
            }

            frame_index += 1;
            if frame_index % 100 == 0 {
                debug!("Frame index: {}", frame_index);
            }
        }
        Ok(())
    };
    tokio::spawn(async {
        let filename = CONFIG.image_filename.clone();
        while !SHUTDOWN.load(Relaxed) {
            SAVE_IMAGE.store(true, Relaxed);
            info!("Waiting for new image...");
            while SAVE_IMAGE.load(Relaxed) {
                sleep(Duration::from_millis(20)).await;
            }
            info!("Got new image!");
            let detection_result = do_detection(&filename).await;

            match detection_result {
                Ok(detection_result) => {
                    debug!("Detection result: {:?}", detection_result);
                    process_detection(&detection_result).await;
                },
                Err(err) => error!("Error while detecting stuff: {}", err),
            }
        }
        mem::drop(filename);
    });

    for (stream, packet) in ictx.packets() {
        if stream.index() == video_stream_index {
            decoder.send_packet(&packet).map_err(|err| format!("Could not send package to decoder: {}", err))?;
            receive_and_process_decoded_frames(&mut decoder).map_err(|err| format!("Could not process decoded frame: {}", err))?;
        }

        if SHUTDOWN.load(Relaxed) {
            break;
        }
    }
    info!("Shutting down");
    SHUTDOWN.store(true, Release);
    decoder.send_eof().map_err(|err| format!("Could not send EOF to stream: {}", err))?;

    Ok(())
}

async fn process_detection(detection_result: &Vec<DetectionResult>) {
    if detection_result.len() == 0 {
        debug!("No detections");
        return;
    }
    info!("Found {} seals", detection_result.len());

    let mut ignore_points_iter = CONFIG.detection.ignore_points.iter();
    let acceptable_detections: Vec<_> = detection_result.iter()
    .filter(|detection| detection.conf > CONFIG.detection.minimum_detection_percentage)
    .filter(|detection| {
        return ignore_points_iter.all(|ignore_point| {
            let x = ignore_point.x as u32;
            let y = ignore_point.y as u32;
            let box_x = detection.r#box[0];
            let box_y = detection.r#box[1];
            let box_width = detection.r#box[2];
            let box_height = detection.r#box[3];
            return x < box_x || x > box_x + box_width || y < box_y || y > box_y + box_height;
        });
    })
    .collect();

    if acceptable_detections.len() == 0 {
        debug!("No acceptable detections");
        return;
    }

    info!("Found {} acceptable detections with confidence threshold of {}", acceptable_detections.len(), CONFIG.detection.minimum_detection_percentage);

    if should_post().await {
        info!("Posting to social media");
        if let Ok(image) = draw_boxes_on_image(&acceptable_detections) {
            if let Err(err) = post_to_social_media(image).await {
                error!("Could not post to social media: {}", err);
            }
        } else {
            error!("Could not draw boxes on image");
        }
    } else if should_save_image().await {
        info!("Posting to social media");
        if let Ok(image) = draw_boxes_on_image(&acceptable_detections) {
            image.save(format!("{}/frame_{}.jpg", CONFIG.output.output_file_folder, Local::now().to_string())).unwrap();
        } else {
            error!("Could not draw boxes on image");
        }
    }

    let _ = draw_boxes_on_image(&acceptable_detections);
}


fn draw_boxes_on_image(acceptable_detections: &Vec<&DetectionResult>) -> Result<DynamicImage, String> {
    let mut image = ImageReader::open(&CONFIG.image_filename)
    .map_err(|err| format!("Could not open image file: {}", err))?
    .decode()
    .map_err(|err| format!("Could not decode image: {}", err))?;

    // Draw bounding boxes
    for detection in acceptable_detections {
        let box_x = detection.r#box[0];
        let box_y = detection.r#box[1];
        let box_width = detection.r#box[2];
        let box_height = detection.r#box[3];
        for i in 0..CONFIG.output.line_thickness {
            imageproc::drawing::draw_hollow_rect_mut(&mut image,
                imageproc::rect::Rect::at(box_x as i32 + i as i32, box_y as i32 + i as i32)
                .of_size(box_width as u32 - 2*i, box_height as u32 - 2*i), image::Rgba(CONFIG.output.line_color));
        }

        // Draw label
        let label = format!("{} ({}%)", detection.cls_name, detection.conf);
        let font = FontRef::try_from_slice(include_bytes!("DejaVuSans.ttf")).map_err(|err| format!("Could not load font: {}", err))?;
        let scale: PxScale = PxScale { x: 25.0, y: 25.0 };
        let text_size = imageproc::drawing::text_size(scale.clone(), &font, &label);
        let padding_x = 10;
        let padding_y = 5;
        let text_x = box_x + box_width - text_size.0 - padding_x;
        if (text_size.1 + 2 * padding_y) < box_y {
            imageproc::drawing::draw_filled_rect_mut(&mut image,
                imageproc::rect::Rect::at((box_x + box_width - text_size.0 - 2 * padding_x) as i32, (box_y - text_size.1 - 2 * padding_y) as i32)
                .of_size(text_size.0 + 2 * padding_x, text_size.1 + 2 * padding_y), image::Rgba(CONFIG.output.line_color));
            imageproc::drawing::draw_text_mut(&mut image, image::Rgba(CONFIG.output.text_color), text_x as i32, (box_y - text_size.1 - padding_y) as i32, scale, &font, label.as_str());
        } else {
        }
    }

    Ok(image)
}

fn get_stream_url(stream_url: &str) -> Result<String, std::io::Error> {
    if !stream_url.starts_with("http") {
        return Ok(stream_url.to_string());
    }

    let output = Command::new("sh")
    .arg("-c")
    .arg(format!("yt-dlp -f 95 -g {}", stream_url))
    .output()
    .expect("failed to execute process");

    Ok(output.stdout.iter().map(|&c| c as char).collect())
}

async fn do_detection(_filename: &str) -> Result<Vec<DetectionResult>, String> {
    info!("Detecting and stuff!");
    sleep(Duration::from_secs(3)).await;
    return Ok(serde_json::from_str("[
        {
            \"box\": [
                100.0,
                500.0,
                190.0,
                192.0
            ],
            \"cls\": 2.0,
            \"cls_name\": \"norppa\",
            \"conf\": 0.7746183276176453
        }
    ]").unwrap());

    // let file_handle = File::open(&filename).await.map_err(|err| format!("Could not open the file: {}", err))?;
    // let bytes_stream = FramedRead::new(file_handle, BytesCodec::new());
    // let form = multipart::Form::new()
    // .part("file", multipart::Part::stream(Body::wrap_stream(bytes_stream))
    // .file_name(filename.to_string())
    // .mime_str("image/png")
    // .expect("error"));
    // println!("File loaded!");
    
    // let client = Client::new();
    // let res = client
    // .post(&CONFIG.detection.api_url,)
    // .multipart(form)
    // .send().await;

    // if let Err(err) = res {
    //     SHUTDOWN.store(true, Release);
    //     return Err(format!("Could not send the request: {}", err));
    // }

    // let result: Vec<DetectionResult> = res.unwrap().json().await
    // .map_err(|err| format!("Could not parse detector response: {}", err))?;

    // println!("{:?}", result);

    // Ok(vec![])
}

fn save_file(frame: &Video, filename: &str) -> Result<(), String> {
    image::save_buffer(format!("{}", filename), frame.data(0), frame.width(), frame.height(), image::ExtendedColorType::Rgb8)
        .map_err(|err| format!("Could not save image: {}", err))?;
    Ok(())
}

// fn add_bounding_boxes_to_image(frame: &mut Video, rects: Vec<u8[4]>) {
//     imageproc::drawing::draw_hollow_rect_mut(&frame, imageproc::rect::Rect::at(0, 0).of_size(100, 100), image::Rgb([255, 0, 0]);
// }
