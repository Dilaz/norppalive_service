use actix::prelude::*;
use image::{DynamicImage, ImageBuffer, Rgb};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tracing::{debug, error, info};

use crate::config::CONFIG;
use crate::error::NorppaliveError;
use crate::messages::supervisor::SystemShutdown;
use crate::messages::{
    DetectorReady, FrameExtracted, GracefulStop, InternalProcessingComplete, LatestFrameAvailable,
    ProcessFrame, StartStream, StopStream,
};

// Production FFmpeg imports
extern crate ffmpeg_next as ffmpeg;
use ffmpeg::codec::context::Context as CodecContext;
use ffmpeg::format::{input, Pixel};
use ffmpeg::media::Type;
use ffmpeg::software::scaling::{Context as ScalingContext, Flags};
use ffmpeg::util::frame::video::Video;
use ffmpeg::Discard;

// Production constants
const MAX_STREAM_ERRORS: u32 = 10;
const MAX_STREAM_RUNTIME: Duration = Duration::from_secs(3600); // Max 1 hour runtime
const STREAM_RUNNING_CHECK_INTERVAL: Duration = Duration::from_secs(30); // Check every 30 seconds

/// StreamActor handles video stream processing and frame extraction
#[derive(Default)]
pub struct StreamActor {
    stream_url: Option<String>,
    running: bool,
    detection_actor: Option<Addr<crate::actors::DetectionActor>>,
    supervisor_actor: Option<Addr<crate::actors::SupervisorActor>>,
    latest_frame_buffer: Option<LatestFrameAvailable>,
    detector_ready: bool,
    shutdown_signal: Option<Arc<AtomicBool>>,
    is_processing_task_running: bool,
    processing_task_handle: Option<JoinHandle<()>>,
}

impl Actor for StreamActor {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        info!("StreamActor started");
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!("StreamActor stopped");

        // Ensure any running task is aborted when the actor stops
        if let Some(signal) = &self.shutdown_signal {
            signal.store(true, Ordering::Relaxed);
        }

        if let Some(task_handle) = self.processing_task_handle.take() {
            info!("Aborting stream processing task during actor shutdown");
            task_handle.abort();
        }
    }
}

impl StreamActor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_actors(
        detection_actor: Addr<crate::actors::DetectionActor>,
        supervisor_actor: Addr<crate::actors::SupervisorActor>,
    ) -> Self {
        Self {
            detection_actor: Some(detection_actor),
            supervisor_actor: Some(supervisor_actor),
            ..Default::default()
        }
    }

    /// Start FFmpeg stream processing in a separate task
    fn start_stream_processing(
        &mut self,
        ctx: &mut Context<Self>,
        stream_url: String,
        init_signal: oneshot::Sender<Result<(), NorppaliveError>>,
    ) {
        let own_addr = ctx.address();

        // Create shutdown signal if not already (should be created per stream start)
        let shutdown_arc = Arc::new(AtomicBool::new(false));
        self.shutdown_signal = Some(shutdown_arc.clone());

        self.is_processing_task_running = true;
        self.running = true;

        info!(target: "stream", "Spawning FFmpeg processing task in blocking thread pool.");

        // Clone the address before moving into spawn_blocking
        let own_addr_for_task = own_addr.clone();
        let own_addr_for_completion = own_addr.clone();

        // Spawn the stream processing task in a blocking thread pool
        let processing_task = async move {
            // Use spawn_blocking for the heavy FFmpeg work
            let processing_result = tokio::task::spawn_blocking(move || {
                Self::process_stream_blocking(
                    stream_url,
                    own_addr_for_task,
                    shutdown_arc,
                    Some(init_signal),
                )
            })
            .await;

            // Handle the result from spawn_blocking
            let final_result = match processing_result {
                Ok(stream_result) => stream_result,
                Err(join_error) => {
                    error!(target: "stream", "FFmpeg processing task panicked or was cancelled: {}", join_error);
                    Err(NorppaliveError::Other(format!(
                        "FFmpeg task failed: {}",
                        join_error
                    )))
                }
            };

            info!(target: "stream", "FFmpeg processing task is sending InternalProcessingComplete to StreamActor.");
            own_addr_for_completion.do_send(InternalProcessingComplete {
                result: final_result,
            });
        };

        let task_handle = actix::spawn(processing_task);
        self.processing_task_handle = Some(task_handle);
    }

    /// Get the stream URL with yt-dlp if it's a YouTube URL
    fn get_stream_url(stream_url: &str) -> Result<String, NorppaliveError> {
        if !stream_url.starts_with("http") {
            return Ok(stream_url.to_string());
        }

        let output_result = Command::new("yt-dlp")
            .args([
                "-f",
                "bestvideo[height<=720]",
                "--js-runtime",
                "bun",
                "-g",
                stream_url,
            ])
            .output();

        match output_result {
            Ok(output) => {
                let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if url.is_empty() {
                    let stderr_output = String::from_utf8_lossy(&output.stderr);
                    error!(
                        target: "stream",
                        "Failed to get stream URL with yt-dlp for {}. yt-dlp stderr: {}",
                        stream_url,
                        stderr_output
                    );
                    Err(NorppaliveError::StreamUrlError(format!(
                        "yt-dlp failed for {}: {}",
                        stream_url,
                        stderr_output.trim()
                    )))
                } else {
                    Ok(url)
                }
            }
            Err(e) => {
                error!(
                    target: "stream",
                    "Failed to execute yt-dlp command for {}: {}",
                    stream_url,
                    e
                );
                Err(NorppaliveError::StreamUrlError(format!(
                    "Failed to execute yt-dlp for {}: {}",
                    stream_url, e
                )))
            }
        }
    }

    /// Sends initialization status via the oneshot channel
    fn send_init_status(
        sender: Option<oneshot::Sender<Result<(), NorppaliveError>>>,
        result: &Result<(), NorppaliveError>,
    ) {
        if let Some(s) = sender {
            let status = result
                .as_ref()
                .map(|_| ())
                .map_err(|err| err.clone_for_error_reporting());
            if let Err(e) = s.send(status) {
                error!(target: "stream", "Failed to send init status: {:?}. Receiver likely dropped.", e);
            }
        }
    }

    /// Initialize FFmpeg and open the stream, returning decoder and scaler
    fn init_ffmpeg_stream(
        stream_url: String,
        init_sender: &mut Option<oneshot::Sender<Result<(), NorppaliveError>>>,
    ) -> Result<
        (
            ffmpeg::format::context::Input,
            ffmpeg::decoder::Video,
            ScalingContext,
            usize,
        ),
        NorppaliveError,
    > {
        if let Err(e) = ffmpeg::init() {
            error!(target: "stream", "FFmpeg init failed: {}", e);
            let err = NorppaliveError::from(e);
            Self::send_init_status(init_sender.take(), &Err(err.clone_for_error_reporting()));
            return Err(err);
        }
        ffmpeg::log::set_level(ffmpeg::log::Level::Quiet);

        // Resolve the actual stream URL (may need yt-dlp for YouTube URLs)
        let actual_url = if stream_url.starts_with("https://") {
            match Self::get_stream_url(&stream_url) {
                Ok(url) => url,
                Err(e) => {
                    error!(target: "stream", "Failed to get stream URL: {}", e);
                    Self::send_init_status(init_sender.take(), &Err(e.clone_for_error_reporting()));
                    return Err(e);
                }
            }
        } else {
            stream_url
        };

        info!(target: "stream", "Attempting to open FFmpeg input for URL: {}", actual_url);
        let ictx = match input(&actual_url) {
            Ok(ctx) => {
                info!(target: "stream", "Successfully opened FFmpeg input for: {}", actual_url);
                Self::send_init_status(init_sender.take(), &Ok(()));
                ctx
            }
            Err(e) => {
                error!(target: "stream", "Failed to open FFmpeg input for {}: {}", actual_url, e);
                let err = NorppaliveError::from(e);
                Self::send_init_status(init_sender.take(), &Err(err.clone_for_error_reporting()));
                return Err(err);
            }
        };

        let input_stream = ictx.streams().best(Type::Video).ok_or_else(|| {
            error!(target: "stream", "Could not find video stream in {}", actual_url);
            NorppaliveError::Other(format!("Could not find video stream in {}", actual_url))
        })?;

        let video_stream_index = input_stream.index();

        let mut decoder = CodecContext::from_parameters(input_stream.parameters())
            .and_then(|context| context.decoder().video())
            .map_err(|e| {
                error!(target: "stream", "Failed to create video decoder: {}", e);
                NorppaliveError::from(e)
            })?;

        if CONFIG.stream.only_keyframes {
            decoder.skip_frame(Discard::NonKey);
        }

        let scaler = ScalingContext::get(
            decoder.format(),
            decoder.width(),
            decoder.height(),
            Pixel::RGB24,
            decoder.width(),
            decoder.height(),
            Flags::BILINEAR,
        )
        .map_err(|e| {
            error!(target: "stream", "Failed to create scaler: {}", e);
            NorppaliveError::from(e)
        })?;

        Ok((ictx, decoder, scaler, video_stream_index))
    }

    /// Process a single decoded frame: scale it and send to the stream actor
    fn process_decoded_frame(
        decoded: &Video,
        scaler: &mut ScalingContext,
        frame_index: u64,
        stream_actor_addr: &Addr<StreamActor>,
        shutdown_signal: &Arc<AtomicBool>,
    ) -> Result<(), u32> {
        // Record start time for minimum processing interval
        let frame_start_time = std::time::Instant::now();

        let mut rgb_frame = Video::empty();
        match scaler.run(decoded, &mut rgb_frame) {
            Ok(_) => {
                // Check shutdown signal before sending frame
                if shutdown_signal.load(Ordering::Relaxed) {
                    info!(target: "stream", "Shutdown signal received before sending frame {}, stopping", frame_index);
                    return Err(0); // Signal to stop, not an error count increment
                }

                // Create and send LatestFrameAvailable to StreamActor
                let timestamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs() as i64;

                let frame_msg = LatestFrameAvailable {
                    frame_data: rgb_frame.data(0).to_vec(),
                    width: rgb_frame.width(),
                    height: rgb_frame.height(),
                    timestamp,
                    frame_index,
                };
                stream_actor_addr.do_send(frame_msg);

                // Ensure minimum processing interval
                let processing_time = frame_start_time.elapsed();
                let min_interval = Duration::from_millis(CONFIG.stream.frame_processing_delay_ms);

                if processing_time < min_interval {
                    let remaining_delay = min_interval - processing_time;
                    debug!(target: "stream",
                        "Frame {} processed in {:.2}ms, sleeping for {:.2}ms to reach minimum interval of {}ms",
                        frame_index,
                        processing_time.as_millis(),
                        remaining_delay.as_millis(),
                        min_interval.as_millis()
                    );
                    std::thread::sleep(remaining_delay);
                } else {
                    debug!(target: "stream",
                        "Frame {} took {:.2}ms to process (>= {}ms minimum), no additional delay needed",
                        frame_index,
                        processing_time.as_millis(),
                        min_interval.as_millis()
                    );
                }

                Ok(())
            }
            Err(e) => {
                error!(target: "stream", "Failed to scale frame {}: {}", frame_index, e);
                Err(1) // Increment error count
            }
        }
    }

    /// Check if the stream processing loop should stop
    fn should_stop_processing(
        shutdown_signal: &Arc<AtomicBool>,
        error_count: u32,
        max_errors: u32,
        start_time: std::time::Instant,
        max_runtime: Duration,
        frame_index: u64,
        max_frames: Option<u64>,
    ) -> Option<&'static str> {
        if shutdown_signal.load(Ordering::Relaxed) {
            return Some("Shutdown signal received");
        }
        if error_count >= max_errors {
            return Some("Too many errors");
        }
        if start_time.elapsed() > max_runtime {
            return Some("Maximum runtime reached");
        }
        if let Some(max) = max_frames {
            if frame_index >= max {
                return Some("Maximum frame count reached");
            }
        }
        None
    }

    /// Main FFmpeg processing function
    fn process_stream_blocking(
        stream_url: String,
        stream_actor_addr: Addr<StreamActor>,
        shutdown_signal: Arc<AtomicBool>,
        init_signal: Option<oneshot::Sender<Result<(), NorppaliveError>>>,
    ) -> Result<(), NorppaliveError> {
        info!(target: "stream", "process_stream_blocking started. Initializing FFmpeg and stream input.");

        let mut init_sender = init_signal;
        let (mut ictx, mut decoder, mut scaler, video_stream_index) =
            Self::init_ffmpeg_stream(stream_url, &mut init_sender)?;

        let mut frame_index = 0u64;
        let mut error_count = 0u32;
        let start_time = std::time::Instant::now();
        let max_frames = CONFIG.stream.max_frames;
        let mut last_running_check = std::time::Instant::now();

        info!(target: "stream", "Starting frame processing loop");

        // Process frames
        for (stream, packet) in ictx.packets() {
            // Check if we should stop processing
            if let Some(reason) = Self::should_stop_processing(
                &shutdown_signal,
                error_count,
                MAX_STREAM_ERRORS,
                start_time,
                MAX_STREAM_RUNTIME,
                frame_index,
                max_frames,
            ) {
                info!(target: "stream", "{}, stopping stream processing", reason);
                break;
            }

            // Periodic status check
            if last_running_check.elapsed() > STREAM_RUNNING_CHECK_INTERVAL {
                if shutdown_signal.load(Ordering::Relaxed) {
                    info!(target: "stream", "Shutdown signal received during periodic check, stopping stream processing");
                    break;
                }
                last_running_check = std::time::Instant::now();
                debug!(target: "stream", "Stream still running, processed {} frames", frame_index);
            }

            if stream.index() != video_stream_index {
                continue;
            }

            // Send packet to decoder
            match decoder.send_packet(&packet) {
                Ok(_) => error_count = 0,
                Err(e) => {
                    error_count += 1;
                    error!(target: "stream", "Failed to send packet to decoder (error {}/{}): {}", error_count, MAX_STREAM_ERRORS, e);
                    if error_count >= MAX_STREAM_ERRORS {
                        break;
                    }
                    continue;
                }
            }

            // Receive and process decoded frames
            let mut decoded = Video::empty();
            while decoder.receive_frame(&mut decoded).is_ok() {
                if shutdown_signal.load(Ordering::Relaxed) {
                    info!(target: "stream", "Shutdown signal received during frame processing, stopping");
                    break;
                }

                frame_index += 1;

                // Process every nth frame based on configuration
                let frame_skip = if CONFIG.stream.only_keyframes { 1 } else { 30 };
                if !frame_index.is_multiple_of(frame_skip) {
                    continue;
                }

                info!(target: "stream", "Processing frame {}", frame_index);

                match Self::process_decoded_frame(
                    &decoded,
                    &mut scaler,
                    frame_index,
                    &stream_actor_addr,
                    &shutdown_signal,
                ) {
                    Ok(()) => {}
                    Err(0) => break, // Shutdown signal
                    Err(inc) => {
                        error_count += inc;
                        if error_count >= MAX_STREAM_ERRORS {
                            error!(target: "stream", "Too many errors ({}/{}), stopping stream processing", error_count, MAX_STREAM_ERRORS);
                            break;
                        }
                    }
                }
            }

            // Check shutdown signal after frame processing
            if shutdown_signal.load(Ordering::Relaxed) {
                info!(target: "stream", "Shutdown signal received after frame processing, stopping stream");
                break;
            }
        }

        // Send EOF to decoder
        if let Err(e) = decoder.send_eof() {
            error!(target: "stream", "Failed to send EOF to decoder: {}", e);
        }

        info!(target: "stream", "Stream processing completed after {} frames in {:.2} seconds.",
              frame_index, start_time.elapsed().as_secs_f64());

        Ok(())
    }

    /// Helper method to send frame to detector when conditions are met
    fn try_send_frame_to_detector(&mut self, ctx: &mut Context<Self>) {
        if self.detector_ready {
            if let Some(frame_details) = self.latest_frame_buffer.take() {
                if let Some(ref det_actor) = self.detection_actor {
                    let image_path = CONFIG.image_filename.clone();

                    // Create DynamicImage from raw frame data
                    let rgb_image: Option<ImageBuffer<Rgb<u8>, Vec<u8>>> = ImageBuffer::from_raw(
                        frame_details.width,
                        frame_details.height,
                        frame_details.frame_data.clone(),
                    );

                    let Some(rgb_image) = rgb_image else {
                        error!(
                            "Failed to create image buffer from frame {} data. Frame dropped.",
                            frame_details.frame_index
                        );
                        return;
                    };

                    let dynamic_image = DynamicImage::ImageRgb8(rgb_image);

                    // Save to disk for detection API (which needs file path)
                    match dynamic_image.save(&image_path) {
                        Ok(_) => {
                            info!(target: "stream", "Saved latest frame {} for detection to {}", frame_details.frame_index, &image_path);

                            // Wrap image in Arc for efficient sharing
                            let image_data = Arc::new(dynamic_image);

                            det_actor.do_send(ProcessFrame {
                                image_data,
                                image_path,
                                timestamp: frame_details.timestamp,
                                reply_to: ctx.address(),
                            });
                            self.detector_ready = false;
                            debug!(
                                "Sent frame {} to detector with in-memory image. Detector is now busy.",
                                frame_details.frame_index
                            );
                        }
                        Err(e) => {
                            error!(
                                "Failed to save frame {} for detection: {}. Frame dropped.",
                                frame_details.frame_index, e
                            );
                            // Frame is dropped. latest_frame_buffer is already None due to take().
                            // Detector remains ready.
                        }
                    }
                } else {
                    debug!(
                        "Detector actor not available, dropping frame {}",
                        frame_details.frame_index
                    );
                    // Frame is dropped.
                }
            } else {
                debug!("Detector ready, but no new frame available in buffer.");
            }
        } else if self.latest_frame_buffer.is_some() {
            debug!(
                "Detector busy. New frame (idx: {}) stored, replacing older buffered frame if any.",
                self.latest_frame_buffer.as_ref().unwrap().frame_index
            );
        } else {
            // This case should ideally not happen if latest_frame_buffer was just set.
            debug!("Detector busy and no new frame to buffer.");
        }
    }

    fn check_and_initiate_shutdown_if_all_done(&mut self, _ctx: &mut Context<Self>) {
        info!(
            target: "stream",
            "Checking shutdown conditions: processing_task_running={}, latest_frame_buffer.is_none()={}, detector_ready={}",
            self.is_processing_task_running,
            self.latest_frame_buffer.is_none(),
            self.detector_ready
        );

        if !self.is_processing_task_running
            && self.latest_frame_buffer.is_none()
            && self.detector_ready
        {
            info!(target: "stream", "All conditions met. Stream processing fully completed. Requesting system shutdown.");
            if let Some(sup_actor) = &self.supervisor_actor {
                sup_actor.do_send(SystemShutdown);
            } else {
                error!(target: "stream", "Supervisor actor not available to request shutdown during final check. Attempting direct stop.");
                // This is a fallback, ideally supervisor_actor should always be present.
                // _ctx.stop(); // Stop self
                // System::current().stop(); // Stop the entire system
            }
        } else {
            debug!(target: "stream", "Shutdown conditions not yet met. Will re-check later.");
        }
    }
}

// =============================================================================
// MESSAGE HANDLERS
// =============================================================================

impl Handler<StartStream> for StreamActor {
    type Result = ResponseFuture<Result<(), NorppaliveError>>;

    fn handle(&mut self, msg: StartStream, ctx: &mut Self::Context) -> Self::Result {
        info!("Received StartStream for: {}", msg.stream_url);

        if self.running {
            info!("Stopping existing stream before starting new one");
            // Signal shutdown to existing stream processing
            if let Some(signal) = &self.shutdown_signal {
                signal.store(true, Ordering::Relaxed);
            }

            // Abort existing task if it's running
            if let Some(task_handle) = self.processing_task_handle.take() {
                info!("Aborting existing stream processing task");
                task_handle.abort();
            }

            self.shutdown_signal = None; // Clear old signal
            self.is_processing_task_running = false;
            // Reset other state related to a running stream if necessary
        }

        self.stream_url = Some(msg.stream_url.clone());
        self.running = true;
        self.latest_frame_buffer = None;
        self.detector_ready = true; // Reset detector state

        let (tx, rx) = oneshot::channel::<Result<(), NorppaliveError>>();

        // The actual stream processing will be started by `start_stream_processing` which itself spawns a future.
        // We need to pass `tx` into that spawned future.
        self.start_stream_processing(ctx, msg.stream_url.clone(), tx);

        let actor_address = ctx.address();

        Box::pin(async move {
            match rx.await {
                Ok(Ok(())) => {
                    info!("Stream initialization reported success by processing task.");
                    Ok(())
                }
                Ok(Err(e)) => {
                    error!("Stream initialization reported failure: {}", e);
                    actor_address.do_send(StreamInitializationFailed);
                    Err(e)
                }
                Err(_channel_error) => {
                    error!("Stream initialization status channel failed (sender dropped).");
                    actor_address.do_send(StreamInitializationFailed);
                    Err(NorppaliveError::Other(
                        "Stream init status channel failed".to_string(),
                    ))
                }
            }
        })
    }
}

// Define StreamInitializationFailed message
#[derive(Message)]
#[rtype(result = "()")]
struct StreamInitializationFailed;

impl Handler<StreamInitializationFailed> for StreamActor {
    type Result = ();

    fn handle(
        &mut self,
        _msg: StreamInitializationFailed,
        _ctx: &mut Context<Self>,
    ) -> Self::Result {
        error!("StreamActor handling StreamInitializationFailed: resetting running state.");
        self.running = false;
        self.stream_url = None;
        self.is_processing_task_running = false;

        // Reset other relevant fields if stream failed to start properly
        if let Some(signal) = self.shutdown_signal.take() {
            signal.store(true, Ordering::Relaxed); // Ensure any nascent task is stopped
        }

        // Abort the task if it's still running
        if let Some(task_handle) = self.processing_task_handle.take() {
            info!("Aborting failed stream processing task");
            task_handle.abort();
        }
    }
}

impl Handler<StopStream> for StreamActor {
    type Result = Result<(), NorppaliveError>;

    fn handle(&mut self, _msg: StopStream, _ctx: &mut Self::Context) -> Self::Result {
        info!("Stopping stream");
        self.running = false;
        self.stream_url = None;
        self.latest_frame_buffer = None;
        self.detector_ready = true;

        // Signal shutdown to stream processing task
        if let Some(signal) = &self.shutdown_signal {
            signal.store(true, Ordering::Relaxed);
            info!("Shutdown signal sent to stream processing task");
        }

        // Abort the processing task if it's still running
        if let Some(task_handle) = self.processing_task_handle.take() {
            info!("Aborting stream processing task");
            task_handle.abort();
        }

        self.shutdown_signal = None; // Clear the signal reference
        self.is_processing_task_running = false;

        Ok(())
    }
}

impl Handler<FrameExtracted> for StreamActor {
    type Result = ();

    fn handle(&mut self, msg: FrameExtracted, _ctx: &mut Self::Context) -> Self::Result {
        info!(
            "Frame extracted: {} bytes at timestamp {}",
            msg.frame_data.len(),
            msg.timestamp
        );
        // This could be used for additional frame processing if needed
    }
}

impl Handler<DetectorReady> for StreamActor {
    type Result = ();

    fn handle(&mut self, _msg: DetectorReady, ctx: &mut Context<Self>) -> Self::Result {
        debug!(target: "stream", "Detector is ready for next frame.");
        self.detector_ready = true;
        self.try_send_frame_to_detector(ctx); // Try to send any buffered frame
        self.check_and_initiate_shutdown_if_all_done(ctx); // Check if all work is done
    }
}

impl Handler<LatestFrameAvailable> for StreamActor {
    type Result = ();

    fn handle(&mut self, msg: LatestFrameAvailable, ctx: &mut Context<Self>) -> Self::Result {
        debug!(target: "stream", "Received latest frame_index: {}", msg.frame_index);
        self.latest_frame_buffer = Some(msg);
        self.try_send_frame_to_detector(ctx); // Try to send immediately if possible
                                              // No explicit shutdown check here, as DetectorReady will trigger it after processing this frame (if sent)
                                              // or InternalProcessingComplete will trigger it if this is the last frame and stream ends.
    }
}

impl Handler<InternalProcessingComplete> for StreamActor {
    type Result = ();

    fn handle(&mut self, msg: InternalProcessingComplete, ctx: &mut Context<Self>) -> Self::Result {
        info!(target: "stream", "Internal FFmpeg processing task reported completion.");
        self.is_processing_task_running = false;
        self.running = false; // Also update the general running flag for the stream
        self.processing_task_handle = None; // Clear the task handle

        if let Err(e) = msg.result {
            error!(target: "stream", "Stream processing task failed: {}. Requesting system shutdown immediately.", e);
            if let Some(sup_actor) = &self.supervisor_actor {
                sup_actor.do_send(SystemShutdown);
            } else {
                error!(target: "stream", "Supervisor actor not available to request shutdown after stream processing error.");
                // Fallback for critical error
                // ctx.stop();
                // System::current().stop();
            }
        } else {
            info!(target: "stream", "Stream source exhausted or task completed. Checking if all frames are processed before shutdown.");
            self.check_and_initiate_shutdown_if_all_done(ctx);
        }
    }
}

impl Handler<GracefulStop> for StreamActor {
    type Result = ();

    fn handle(&mut self, msg: GracefulStop, ctx: &mut Context<Self>) -> Self::Result {
        info!(target: "stream", "StreamActor: Received GracefulStop message. Stopping stream processing.");

        // Signal shutdown to stream processing task
        if let Some(signal) = &self.shutdown_signal {
            signal.store(true, Ordering::Relaxed);
            info!(target: "stream", "StreamActor: Shutdown signal sent to stream processing task");
        }

        // Abort the processing task if it's still running
        if let Some(task_handle) = self.processing_task_handle.take() {
            info!(target: "stream", "StreamActor: Aborting stream processing task");
            task_handle.abort();
        }

        self.running = false;
        self.is_processing_task_running = false;

        // Stop the actor
        ctx.stop();

        // Send acknowledgment
        if let Some(ack_sender) = msg.ack_sender {
            let _ = ack_sender.send(());
            info!(target: "stream", "StreamActor: Sent shutdown acknowledgment");
        }
    }
}
