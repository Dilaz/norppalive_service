extern crate ffmpeg_next as ffmpeg;

use std::mem;
use std::time::Duration;

use clap::{command, Parser};
use ffmpeg::format::{input, Pixel};
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
use miette::Result;

pub mod config;
pub mod utils;
pub mod services;
pub mod error;

use error::NorppaliveError;
use config::Config;

#[derive(Parser)]
#[command(version, about, long_about = None, name = "Norppalive Service", author = "Risto \"Dilaz\" Viitanen")]
struct Args {
    #[arg(short, long, default_value = "config.toml")]
    config: String,
}

lazy_static! {
    static ref ARGS: Args                      = Args::parse();
    static ref SHUTDOWN: AtomicBool            = AtomicBool::new(false);
    static ref CONFIG: Config                  = toml::from_str(&std::fs::read_to_string(&ARGS.config).unwrap()).unwrap();
    static ref SAVE_IMAGE: AtomicBool          = AtomicBool::new(true);
    static ref LAST_POST_TIME: AtomicI64       = AtomicI64::new(0);
    static ref LAST_IMAGE_SAVE_TIME: AtomicI64 = AtomicI64::new(0);
}

#[tokio::main]
async fn main() -> Result<(), NorppaliveError> {
    tracing::info!("Starting!");
    // initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "norppalive_service=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    ffmpeg::init()?;

    // Set log level to suppress FFmpeg logs
    ffmpeg::log::set_level(ffmpeg::log::Level::Quiet);

    // Get the actual stream url from Youtube using yt-dlp
    let stream_url = if CONFIG.stream.stream_url.starts_with("https://") {
        get_stream_url(&CONFIG.stream.stream_url)?
    } else {
        CONFIG.stream.stream_url.clone()
    };
    if stream_url.is_empty() {
        return Err(NorppaliveError::Other("Could not get stream url".to_string()));
    }
    info!("Stream URL: {}", stream_url);

    // Select the best video quality stream
    let mut ictx = input(&stream_url)?;
    let input = ictx
        .streams()
        .best(Type::Video)
        .ok_or(NorppaliveError::Other("Could not open the stream".to_string()))?;

    let video_stream_index = input.index();

    // Create a decoder
    let mut decoder = input.codec().decoder().video()?;

    // We can skip everything else and only detect from keyframes (which is recommended)
    if CONFIG.stream.only_keyframes {
        decoder.skip_frame(Discard::NonKey);
    }

    let mut scaler = Context::get(
        decoder.format(),
        decoder.width(),
        decoder.height(),
        Pixel::RGB24,
        decoder.width(),
        decoder.height(),
        Flags::BILINEAR,
    )?;

    let mut frame_index = 0u64;
    let mut skipped = 0u64;

    // Callback function for the received frames
    let mut receive_and_process_decoded_frames =
    |decoder: &mut ffmpeg::decoder::Video| -> Result<(), NorppaliveError> {
        let mut decoded = Video::empty();
        while decoder.receive_frame(&mut decoded).is_ok() {
            if SAVE_IMAGE.load(Relaxed) {
                info!("Saving image");
                let mut rgb_frame = Video::empty();
                scaler.run(&decoded, &mut rgb_frame)?;
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

    // Spawn a new thread to do the detections in so it doesn't mess with ffmpeg
    let detection_thread: tokio::task::JoinHandle<Result<(), NorppaliveError>> = tokio::spawn(async {
        // Create a detection service
        let mut detection_service = utils::detection_utils::DetectionService::default();
        let filename = CONFIG.image_filename.clone();
        let mut detections_in_row = 0;

        // If global SHUTDOWN is set, just stop
        while !SHUTDOWN.load(Relaxed) {
            SAVE_IMAGE.store(true, Relaxed);
            info!("Waiting for new image...");
            while SAVE_IMAGE.load(Relaxed) {
                sleep(Duration::from_millis(20)).await;
            }
            info!("Got new image!");
            let detection_result = detection_service.do_detection(&filename).await?;
            debug!("Detection result: {:?}", detection_result);

            // Update detection count
            detections_in_row = detection_service.process_detection(&detection_result, detections_in_row).await.unwrap_or(0);
            debug!("Detections in row: {}", detections_in_row);
        }
        mem::drop(filename);

        Ok(())
    });

    // Start reading packets from the stream
    for (stream, packet) in ictx.packets() {
        if stream.index() == video_stream_index {
            decoder.send_packet(&packet)?;
            receive_and_process_decoded_frames(&mut decoder)?;
        }

        if SHUTDOWN.load(Relaxed) {
            break;
        }
    }
    info!("Shutting down");
    SHUTDOWN.store(true, Release);

    // Wait for the detection thread to shut down
    if let Err(e) = detection_thread.await {
        error!("Detection thread error: {:?}", e);
    }

    // Send EOF signal to the decoder
    decoder.send_eof()?;

    Ok(())
}



/**
 * Get the stream URL with yt-dlp
 */
fn get_stream_url(stream_url: &str) -> Result<String, NorppaliveError> {
    if !stream_url.starts_with("http") {
        return Ok(stream_url.to_string());
    }

    let output = Command::new("sh")
    .arg("-c")
    .arg(format!("yt-dlp -f 95 -g {}", stream_url))
    .output()?;

    Ok(output.stdout.iter().map(|&c| c as char).collect())
}



/**
 * Save the frame to a file
 */
fn save_file(frame: &Video, filename: &str) -> Result<(), NorppaliveError> {
    image::save_buffer(filename, frame.data(0), frame.width(), frame.height(), image::ExtendedColorType::Rgb8)?;
    Ok(())
}
