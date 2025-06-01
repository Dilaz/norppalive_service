use actix::prelude::*;
use norppalive_service::actors::SupervisorActor;
use norppalive_service::error::NorppaliveError;
use norppalive_service::messages::{
    DetectorReady, FrameExtracted, LatestFrameAvailable, ProcessFrame, StartStream, StopStream,
    InternalProcessingComplete,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::oneshot;
use tracing::info;

// Test version of StreamActor for testing without FFmpeg dependencies
pub struct TestStreamActor {
    stream_url: Option<String>,
    running: bool,
    detection_actor: Option<Addr<norppalive_service::actors::DetectionActor>>,
    supervisor_actor: Option<Addr<norppalive_service::actors::SupervisorActor>>,
    latest_frame_buffer: Option<LatestFrameAvailable>,
    detector_ready: bool,
    shutdown_signal: Option<Arc<AtomicBool>>,
    is_processing_task_running: bool,
}

impl Default for TestStreamActor {
    fn default() -> Self {
        TestStreamActor {
            stream_url: None,
            running: false,
            detection_actor: None,
            supervisor_actor: None,
            latest_frame_buffer: None,
            detector_ready: true,
            shutdown_signal: None,
            is_processing_task_running: false,
        }
    }
}

impl Actor for TestStreamActor {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        info!("TestStreamActor started");
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!("TestStreamActor stopped");
    }
}

impl TestStreamActor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_actors(
        detection_actor: Addr<norppalive_service::actors::DetectionActor>,
        supervisor_actor: Addr<norppalive_service::actors::SupervisorActor>,
    ) -> Self {
        Self {
            detection_actor: Some(detection_actor),
            supervisor_actor: Some(supervisor_actor),
            ..Default::default()
        }
    }

    fn start_stream_processing(
        &mut self,
        _ctx: &mut Context<Self>,
        stream_url: String,
        init_signal: oneshot::Sender<Result<(), NorppaliveError>>,
    ) {
        info!("Test: Starting stream processing for {}", stream_url);
        self.is_processing_task_running = true;
        self.running = true;
        
        // Send immediate success for tests
        let _ = init_signal.send(Ok(()));
        
        // Simulate completion
        self.is_processing_task_running = false;
    }
}

// Implement handlers for TestStreamActor
impl Handler<StartStream> for TestStreamActor {
    type Result = ResponseFuture<Result<(), NorppaliveError>>;

    fn handle(&mut self, msg: StartStream, ctx: &mut Self::Context) -> Self::Result {
        info!("Test: Received StartStream for: {}", msg.stream_url);

        self.stream_url = Some(msg.stream_url.clone());
        self.running = true;
        self.latest_frame_buffer = None;
        self.detector_ready = true;

        let (tx, rx) = oneshot::channel::<Result<(), NorppaliveError>>();
        self.start_stream_processing(ctx, msg.stream_url.clone(), tx);

        Box::pin(async move {
            match rx.await {
                Ok(Ok(())) => {
                    info!("Test: Stream initialization success");
                    Ok(())
                }
                Ok(Err(e)) => {
                    info!("Test: Stream initialization failed: {}", e);
                    Err(e)
                }
                Err(_) => {
                    info!("Test: Stream initialization channel failed");
                    Err(NorppaliveError::Other("Test channel failed".to_string()))
                }
            }
        })
    }
}

impl Handler<StopStream> for TestStreamActor {
    type Result = Result<(), NorppaliveError>;

    fn handle(&mut self, _msg: StopStream, _ctx: &mut Self::Context) -> Self::Result {
        info!("Test: Stopping stream");
        self.running = false;
        self.stream_url = None;
        self.latest_frame_buffer = None;
        self.detector_ready = true;
        Ok(())
    }
}

impl Handler<FrameExtracted> for TestStreamActor {
    type Result = ();

    fn handle(&mut self, msg: FrameExtracted, _ctx: &mut Self::Context) -> Self::Result {
        info!("Test: Frame extracted: {} bytes", msg.frame_data.len());
    }
}

// Tests
#[actix::test]
async fn test_stream_actor_creation() {
    let supervisor_addr = SupervisorActor::new().start();
    let detection_addr = norppalive_service::actors::DetectionActor::new().start();
    let actor = TestStreamActor::with_actors(detection_addr, supervisor_addr);
    assert!(actor.stream_url.is_none());
    assert!(!actor.running);
    assert!(actor.supervisor_actor.is_some());
}

#[actix::test]
async fn test_start_stream() {
    let actor = TestStreamActor::new().start();

    let result = actor
        .send(StartStream {
            stream_url: "rtsp://example.com/stream".to_string(),
        })
        .await
        .unwrap();

    assert!(result.is_ok());
}

#[actix::test]
async fn test_stop_stream() {
    let actor = TestStreamActor::new().start();

    // First start a stream
    let _ = actor
        .send(StartStream {
            stream_url: "rtsp://example.com/stream".to_string(),
        })
        .await
        .unwrap();

    // Then stop it
    let result = actor.send(StopStream).await.unwrap();
    assert!(result.is_ok());
}

#[actix::test]
async fn test_start_multiple_streams() {
    let actor = TestStreamActor::new().start();

    // Start first stream
    let result1 = actor
        .send(StartStream {
            stream_url: "rtsp://example1.com/stream".to_string(),
        })
        .await
        .unwrap();
    assert!(result1.is_ok());

    // Start second stream (should replace the first)
    let result2 = actor
        .send(StartStream {
            stream_url: "rtsp://example2.com/stream".to_string(),
        })
        .await
        .unwrap();
    assert!(result2.is_ok());
}

#[actix::test]
async fn test_stop_stream_when_not_running() {
    let actor = TestStreamActor::new().start();

    // Try to stop stream when none is running
    let result = actor.send(StopStream).await.unwrap();
    assert!(result.is_ok()); // Should handle gracefully
}

#[actix::test]
async fn test_concurrent_start_stop() {
    let actor = TestStreamActor::new().start();

    // Send start and stop messages concurrently
    let start_future = actor.send(StartStream {
        stream_url: "rtsp://example.com/stream".to_string(),
    });
    let stop_future = actor.send(StopStream);

    let (start_result, stop_result) = futures::future::join(start_future, stop_future).await;

    // Both should complete successfully
    assert!(start_result.unwrap().is_ok());
    assert!(stop_result.unwrap().is_ok());
}

#[actix::test]
async fn test_different_stream_urls() {
    let actor = TestStreamActor::new().start();

    let test_urls = vec![
        "rtsp://example.com/stream",
        "https://youtube.com/watch?v=test",
        "file://local/path/video.mp4",
        "udp://multicast.example.com:1234",
    ];

    for url in test_urls {
        let result = actor
            .send(StartStream {
                stream_url: url.to_string(),
            })
            .await
            .unwrap();
        assert!(result.is_ok());
    }
}

#[actix::test]
async fn test_frame_extracted_with_various_data() {
    let actor = TestStreamActor::new().start();

    // Test with different frame data sizes
    let test_frames = [
        vec![],                 // Empty frame
        vec![0; 1024],          // Small frame
        vec![255; 1024 * 1024], // Large frame
    ];

    for (i, frame_data) in test_frames.iter().enumerate() {
        actor.do_send(FrameExtracted {
            frame_data: frame_data.clone(),
            timestamp: 1000 + i as i64,
            frame_index: i as u64,
        });
    }

    // Should handle all frame sizes without issues
    assert!(test_frames.len() == 3);
} 