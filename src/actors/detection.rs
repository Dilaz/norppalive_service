use actix::prelude::*;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info};

use crate::actors::OutputActor;
use crate::error::NorppaliveError;
use crate::messages::{
    DetectionCompleted, DetectionStats, DetectorReady, GetDetectionStats, ProcessFrame,
};
use crate::utils::detection_utils::{DetectionResult, DetectionService, DetectionServiceTrait};

#[cfg(test)]
use crate::utils::detection_utils::MockDetectionService;

/// DetectionActor handles image detection processing and analysis
pub struct DetectionActor {
    detection_service: Arc<Mutex<dyn DetectionServiceTrait + Send>>,
    total_detections: u64,
    consecutive_detections: u32,
    last_detection_time: i64,
    output_actor: Option<Addr<OutputActor>>,
}

impl Default for DetectionActor {
    fn default() -> Self {
        Self {
            detection_service: Arc::new(Mutex::new(DetectionService::default())),
            total_detections: 0,
            consecutive_detections: 0,
            last_detection_time: 0,
            output_actor: None,
        }
    }
}

impl Actor for DetectionActor {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        info!("DetectionActor started");
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!("DetectionActor stopped");
    }
}

impl DetectionActor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_output_actor(output_actor: Addr<OutputActor>) -> Self {
        Self {
            output_actor: Some(output_actor),
            ..Self::default()
        }
    }

    #[cfg(test)]
    pub fn new_with_service(service: Arc<Mutex<dyn DetectionServiceTrait + Send>>) -> Self {
        Self {
            detection_service: service,
            total_detections: 0,
            consecutive_detections: 0,
            last_detection_time: 0,
            output_actor: None,
        }
    }

    fn update_detection_state(
        &mut self,
        timestamp: i64,
        detection_result: &Result<Vec<DetectionResult>, NorppaliveError>,
    ) {
        match detection_result {
            Ok(detections) => {
                self.total_detections += detections.len() as u64;
                self.last_detection_time = timestamp;
                if !detections.is_empty() {
                    self.consecutive_detections += 1;
                } else {
                    self.consecutive_detections = 0;
                }
            }
            Err(err) => {
                error!("Error processing detection result: {}", err);
                self.consecutive_detections = 0;
            }
        }
    }
}

impl Handler<ProcessFrame> for DetectionActor {
    type Result = ResponseActFuture<Self, Result<Vec<DetectionResult>, NorppaliveError>>;

    fn handle(&mut self, msg: ProcessFrame, _ctx: &mut Self::Context) -> Self::Result {
        info!("Processing frame: {}", msg.image_path);

        let image_path = msg.image_path.clone();
        let image_path_for_output = msg.image_path.clone();
        let timestamp = msg.timestamp;
        let reply_to_addr = msg.reply_to.clone();
        let detection_service = self.detection_service.clone();
        let output_actor = self.output_actor.clone();

        Box::pin(
            async move {
                let mut service_instance = detection_service.lock().await;
                let detection_result = service_instance.do_detection(&image_path).await;

                // Update temperature map with detections
                if let Ok(detections) = &detection_result {
                    service_instance.update_temperature_map(detections);
                }

                // Filter detections to only include high-confidence, valid ones
                let filtered_detections = if let Ok(detections) = &detection_result {
                    service_instance
                        .filter_acceptable_detections(detections)
                        .into_iter()
                        .cloned()
                        .collect::<Vec<_>>()
                } else {
                    vec![]
                };

                (detection_result, filtered_detections)
            }
            .into_actor(self)
            .map(move |(result, filtered_detections), actor, _ctx| {
                // Update state based on FILTERED detections (not raw)
                let filtered_result = Ok(filtered_detections.clone());
                actor.update_detection_state(timestamp, &filtered_result);
                reply_to_addr.do_send(DetectorReady);

                // Send DetectionCompleted to OutputActor with FILTERED detections only
                if !filtered_detections.is_empty() {
                    if let Some(ref output) = output_actor {
                        output.do_send(DetectionCompleted {
                            detections: filtered_detections,
                            timestamp,
                            consecutive_detections: actor.consecutive_detections,
                            image_path: image_path_for_output,
                        });
                    }
                }

                result
            }),
        )
    }
}

impl Handler<GetDetectionStats> for DetectionActor {
    type Result = ResponseActFuture<Self, Result<DetectionStats, NorppaliveError>>;

    fn handle(&mut self, _msg: GetDetectionStats, _ctx: &mut Self::Context) -> Self::Result {
        let detection_service_arc = self.detection_service.clone();

        Box::pin(
            async move {
                let service_guard = detection_service_arc.lock().await;
                let has_hotspots = service_guard.has_heatmap_hotspots(85.0);
                Ok::<bool, NorppaliveError>(has_hotspots)
            }
            .into_actor(self)
            .map(
                |has_hotspots_result, actor, _ctx| match has_hotspots_result {
                    Ok(hotspots) => Ok(DetectionStats {
                        total_detections: actor.total_detections,
                        consecutive_detections: actor.consecutive_detections,
                        last_detection_time: actor.last_detection_time,
                        temperature_map_hotspots: hotspots,
                    }),
                    Err(e) => {
                        error!("Failed to get detection stats (heatmap status): {}", e);
                        Err(NorppaliveError::Other(format!(
                            "Failed to get heatmap status: {}",
                            e
                        )))
                    }
                },
            ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actors::StreamActor;
    use std::fs;
    use std::io::Write;
    use tempfile;

    fn create_test_detection() -> DetectionResult {
        DetectionResult {
            r#box: [10, 10, 50, 50],
            cls: 0,
            cls_name: "seal".to_string(),
            conf: 85,
        }
    }

    #[actix::test]
    async fn test_detection_actor_creation() {
        let actor_default = DetectionActor::new();
        assert_eq!(actor_default.total_detections, 0);

        let mock_service = Arc::new(Mutex::new(MockDetectionService::new()));
        let actor = DetectionActor::new_with_service(mock_service);
        assert_eq!(actor.total_detections, 0);
        assert_eq!(actor.consecutive_detections, 0);
        assert_eq!(actor.last_detection_time, 0);
    }

    #[actix::test]
    async fn test_detection_actor_startup() {
        let mock_service_impl = MockDetectionService::new().with_hotspots(true);
        let mock_service = Arc::new(Mutex::new(mock_service_impl));
        let actor = DetectionActor::new_with_service(mock_service).start();

        let stats = actor.send(GetDetectionStats).await.unwrap().unwrap();
        assert_eq!(stats.total_detections, 0);
        assert_eq!(stats.consecutive_detections, 0);
        assert_eq!(stats.last_detection_time, 0);
        assert!(stats.temperature_map_hotspots);
    }

    #[actix::test]
    async fn test_get_detection_stats() {
        let mock_service_impl = MockDetectionService::new().with_hotspots(false);
        let mock_service = Arc::new(Mutex::new(mock_service_impl));
        let actor = DetectionActor::new_with_service(mock_service).start();

        let stats = actor.send(GetDetectionStats).await.unwrap().unwrap();

        assert_eq!(stats.total_detections, 0);
        assert_eq!(stats.consecutive_detections, 0);
        assert_eq!(stats.last_detection_time, 0);
        assert!(!stats.temperature_map_hotspots);
    }

    #[actix::test]
    async fn test_process_frame_with_invalid_image() {
        let mock_detection_service = MockDetectionService::new();
        let mock_service_arc = Arc::new(Mutex::new(mock_detection_service));
        let actor = DetectionActor::new_with_service(mock_service_arc.clone()).start();
        let mock_stream_actor_addr = StreamActor::new().start();

        let result = actor
            .send(ProcessFrame {
                image_path: "/nonexistent/path/image.jpg".to_string(),
                timestamp: 1234567890,
                reply_to: mock_stream_actor_addr.clone(),
            })
            .await
            .unwrap();

        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 0);

        let stats = actor.send(GetDetectionStats).await.unwrap().unwrap();
        assert_eq!(stats.consecutive_detections, 0);
        assert_eq!(stats.last_detection_time, 1234567890);
    }

    #[actix::test]
    async fn test_process_frame_with_detections() {
        let detection1 = create_test_detection();
        let mock_detection_service =
            MockDetectionService::new().with_mock_detections(vec![detection1.clone()]);
        let mock_service_arc = Arc::new(Mutex::new(mock_detection_service));
        let actor = DetectionActor::new_with_service(mock_service_arc.clone()).start();
        let mock_stream_actor_addr = StreamActor::new().start();

        let temp_dir = tempfile::tempdir().unwrap();
        let image_path = temp_dir.path().join("test_image_with_detection.jpg");
        let mut file = fs::File::create(&image_path).unwrap();
        file.write_all(b"fake image data").unwrap();

        let result = actor
            .send(ProcessFrame {
                image_path: image_path.to_string_lossy().to_string(),
                timestamp: 100,
                reply_to: mock_stream_actor_addr.clone(),
            })
            .await
            .unwrap();

        assert!(result.is_ok());
        let detections = result.unwrap();
        assert_eq!(detections.len(), 1);
        assert_eq!(detections[0].cls_name, "seal");

        let stats = actor.send(GetDetectionStats).await.unwrap().unwrap();
        assert_eq!(stats.total_detections, 1);
        assert_eq!(stats.consecutive_detections, 1);
        assert_eq!(stats.last_detection_time, 100);

        let mock_detection_service_empty = MockDetectionService::new();
        {
            let mut service_guard = mock_service_arc.lock().await;
            *service_guard = mock_detection_service_empty;
        }

        let image_path_no_detection = temp_dir.path().join("no_detection.jpg");
        let mut file_no_detection = fs::File::create(&image_path_no_detection).unwrap();
        file_no_detection.write_all(b"other fake data").unwrap();

        let _ = actor
            .send(ProcessFrame {
                image_path: image_path_no_detection.to_string_lossy().to_string(),
                timestamp: 200,
                reply_to: mock_stream_actor_addr.clone(),
            })
            .await
            .unwrap();

        let stats_after_empty = actor.send(GetDetectionStats).await.unwrap().unwrap();
        assert_eq!(stats_after_empty.total_detections, 1);
        assert_eq!(stats_after_empty.consecutive_detections, 0);
        assert_eq!(stats_after_empty.last_detection_time, 200);
    }

    #[actix::test]
    async fn test_process_frame_with_valid_image_no_detections() {
        let mock_detection_service = MockDetectionService::new();
        let mock_service_arc = Arc::new(Mutex::new(mock_detection_service));
        let actor = DetectionActor::new_with_service(mock_service_arc.clone()).start();
        let mock_stream_actor_addr = StreamActor::new().start();

        let temp_dir = tempfile::tempdir().unwrap();
        let image_path = temp_dir.path().join("test_image.jpg");
        let mut file = fs::File::create(&image_path).unwrap();
        file.write_all(b"fake image data for testing").unwrap();

        let result = actor
            .send(ProcessFrame {
                image_path: image_path.to_string_lossy().to_string(),
                timestamp: 1234567890,
                reply_to: mock_stream_actor_addr.clone(),
            })
            .await
            .unwrap();

        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 0);

        let stats = actor.send(GetDetectionStats).await.unwrap().unwrap();
        assert_eq!(stats.consecutive_detections, 0);
        assert_eq!(stats.last_detection_time, 1234567890);
    }

    #[actix::test]
    async fn test_detection_stats_updates() {
        let mock_service = Arc::new(Mutex::new(MockDetectionService::new()));
        let mut actor = DetectionActor::new_with_service(mock_service);

        actor.total_detections = 5;
        actor.consecutive_detections = 2;
        actor.last_detection_time = 9876543210;

        let actor_addr = actor.start();

        let stats = actor_addr.send(GetDetectionStats).await.unwrap().unwrap();
        assert_eq!(stats.total_detections, 5);
        assert_eq!(stats.consecutive_detections, 2);
        assert_eq!(stats.last_detection_time, 9876543210);
    }

    #[actix::test]
    async fn test_multiple_detection_requests() {
        let mock_service_arc = Arc::new(Mutex::new(MockDetectionService::new()));
        let actor = DetectionActor::new_with_service(mock_service_arc.clone()).start();
        let mock_stream_actor_addr = StreamActor::new().start();

        let results = futures::future::join_all(vec![
            actor.send(ProcessFrame {
                image_path: "/fake/path1.jpg".to_string(),
                timestamp: 1000,
                reply_to: mock_stream_actor_addr.clone(),
            }),
            actor.send(ProcessFrame {
                image_path: "/fake/path2.jpg".to_string(),
                timestamp: 2000,
                reply_to: mock_stream_actor_addr.clone(),
            }),
            actor.send(ProcessFrame {
                image_path: "/fake/path3.jpg".to_string(),
                timestamp: 3000,
                reply_to: mock_stream_actor_addr.clone(),
            }),
        ])
        .await;

        for mail_result in results {
            let result = mail_result.unwrap();
            assert!(result.is_ok());
            assert_eq!(result.unwrap().len(), 0);
        }

        let stats = actor.send(GetDetectionStats).await.unwrap().unwrap();
        assert_eq!(stats.consecutive_detections, 0);
        assert_eq!(stats.total_detections, 0);
        assert!(
            stats.last_detection_time == 1000
                || stats.last_detection_time == 2000
                || stats.last_detection_time == 3000
        );
    }
}
