extern crate ffmpeg_next as ffmpeg;

use std::mem;
use std::time::Duration;

use clap::{command, Parser};
use ffmpeg::format::{input, Pixel};
use ffmpeg::media::Type;
use ffmpeg::software::scaling::{Context, Flags};
use ffmpeg::util::frame::video::Video;
use ffmpeg::Discard;
use lazy_static::lazy_static;
use miette::Result;
use std::process::Command;
use std::sync::atomic::{
    AtomicBool, AtomicI64,
    Ordering::{Relaxed, Release},
};
use tokio::time::sleep;
use tracing::{debug, error, info};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

pub mod config;
pub mod error;
pub mod services;
pub mod utils;

use config::Config;
use error::NorppaliveError;

#[derive(Parser)]
#[command(version, about, long_about = None, name = "Norppalive Service", author = "Risto \"Dilaz\" Viitanen")]
struct Args {
    #[arg(short, long, default_value = "config.toml")]
    config: String,
}

lazy_static! {
    static ref ARGS: Args = Args::parse();
    static ref SHUTDOWN: AtomicBool = AtomicBool::new(false);
    static ref CONFIG: Config =
        toml::from_str(&std::fs::read_to_string(&ARGS.config).unwrap()).unwrap();
    static ref SAVE_IMAGE: AtomicBool = AtomicBool::new(true);
    static ref LAST_POST_TIME: AtomicI64 = AtomicI64::new(0);
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
        return Err(NorppaliveError::Other(
            "Could not get stream url".to_string(),
        ));
    }
    info!("Stream URL: {}", stream_url);

    // Select the best video quality stream
    let mut ictx = input(&stream_url)?;
    let input = ictx
        .streams()
        .best(Type::Video)
        .ok_or(NorppaliveError::Other(
            "Could not open the stream".to_string(),
        ))?;

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
    let detection_thread: tokio::task::JoinHandle<Result<(), NorppaliveError>> =
        tokio::spawn(async {
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
                if let Ok(detection_result) = detection_service.do_detection(&filename).await {
                    debug!("Detection result: {:?}", detection_result);

                    // Update detection count
                    detections_in_row = detection_service
                        .process_detection(&detection_result, detections_in_row)
                        .await
                        .unwrap_or(0);
                    debug!("Detections in row: {}", detections_in_row);
                }
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

    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if url.is_empty() {
        return Err(NorppaliveError::Other(
            "Failed to get stream URL".to_string(),
        ));
    }

    Ok(url)
}

/**
 * Save the frame to a file
 */
fn save_file(frame: &Video, filename: &str) -> Result<(), NorppaliveError> {
    if frame.data(0).is_empty() {
        return Err(NorppaliveError::Other("Frame data is empty".to_string()));
    }
    image::save_buffer(
        filename,
        frame.data(0),
        frame.width(),
        frame.height(),
        image::ExtendedColorType::Rgb8,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Read;

    #[test]
    fn test_get_stream_url() {
        // Testing the non-HTTP case which doesn't use yt-dlp
        let url = "rtsp://example.com/stream";
        let result = get_stream_url(url);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), url);
    }

    #[test]
    fn test_save_file() {
        // Use a timestamp to ensure unique filename for parallel test runs
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let filename = format!("test_output_{}.png", timestamp);

        // Create an empty frame
        let mut frame = Video::empty();

        // Set frame properties
        let width = 640;
        let height = 480;

        // Allocate the frame with proper format
        unsafe {
            frame.alloc(Pixel::RGB24, width, height);
        }

        // Fill with test data (black image)
        let data = vec![0; (width * height * 3) as usize];

        // Copy data to the frame's buffer
        let stride = frame.stride(0);
        for y in 0..height {
            for x in 0..width {
                let frame_offset = y as usize * stride + x as usize * 3;
                let data_offset = (y * width + x) as usize * 3;

                if frame_offset + 2 < stride * height as usize && data_offset + 2 < data.len() {
                    frame.data_mut(0)[frame_offset] = data[data_offset];
                    frame.data_mut(0)[frame_offset + 1] = data[data_offset + 1];
                    frame.data_mut(0)[frame_offset + 2] = data[data_offset + 2];
                }
            }
        }

        // Save the frame to a file
        let result = save_file(&frame, &filename);
        assert!(result.is_ok());

        // Verify file exists and has content
        let mut file = File::open(&filename).unwrap();
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer).unwrap();
        assert!(!buffer.is_empty());

        // Clean up - remove the test file
        std::fs::remove_file(&filename).unwrap_or_else(|e| {
            println!("Warning: Could not remove test file {}: {}", filename, e);
        });
    }
}
