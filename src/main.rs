extern crate ffmpeg_next as ffmpeg;

use std::mem;
use std::time::Duration;

use clap::{command, Parser};
use ffmpeg::format::{input, stream, Pixel};
use ffmpeg::media::Type;
use ffmpeg::software::scaling::{Context, Flags};
use ffmpeg::util::frame::video::Video;
use ffmpeg::Discard;
use tokio::time::sleep;
use tracing_subscriber::layer::SubscriberExt;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering::{Relaxed, Release}, AtomicI64};
use tracing::{debug, error, info};
use tracing_subscriber::util::SubscriberInitExt;
use lazy_static::lazy_static;

pub mod config;
pub mod utils;
pub mod services;

use config::Config;


#[derive(Parser)]
#[command(version, about, long_about = None, name = "Norppalive Service", author = "Risto \"Dilaz\" Viitanen")]
struct Args {
    #[arg(short, long, default_value = "config.toml")]
    config: String,
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
    if stream_url.is_empty() {
        return Err("Could not get stream url".to_string());
    }
    info!("Stream URL: {}", stream_url);
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
        // Create a detection service
        let mut detection_service = utils::detection_utils::DetectionService::new();
        let filename = CONFIG.image_filename.clone();
        while !SHUTDOWN.load(Relaxed) {
            SAVE_IMAGE.store(true, Relaxed);
            info!("Waiting for new image...");
            while SAVE_IMAGE.load(Relaxed) {
                sleep(Duration::from_millis(20)).await;
            }
            info!("Got new image!");
            let detection_result = detection_service.do_detection(&filename).await;

            match detection_result {
                Ok(detection_result) => {
                    debug!("Detection result: {:?}", detection_result);
                    detection_service.process_detection(&detection_result).await;
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



/**
 * Get the stream URL with yt-dlp
 */
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



/**
 * Save the frame to a file
 */
fn save_file(frame: &Video, filename: &str) -> Result<(), String> {
    image::save_buffer(format!("{}", filename), frame.data(0), frame.width(), frame.height(), image::ExtendedColorType::Rgb8)
        .map_err(|err| format!("Could not save image: {}", err))?;
    Ok(())
}
