use actix::prelude::*;
use tracing::{debug, info, warn};

use crate::actors::services::{BlueskyActor, KafkaActor, MastodonActor, TwitterActor};
use crate::config::CONFIG;
use crate::error::NorppaliveError;
use crate::messages::{
    DetectionCompleted, GetServiceStatus, PostToSocialMedia, SaveDetectionImage,
    SaveHeatmapVisualization, ServicePost, ServiceStatus,
};
use crate::utils::image_utils::draw_boxes_on_provided_image;
use crate::utils::output::OutputService;
use crate::utils::output::OutputServiceTrait;

/// OutputActor coordinates output operations (posting, saving)
pub struct OutputActor {
    output_service: Box<dyn OutputServiceTrait>,
    twitter_actor: Option<Addr<TwitterActor>>,
    bluesky_actor: Option<Addr<BlueskyActor>>,
    mastodon_actor: Option<Addr<MastodonActor>>,
    kafka_actor: Option<Addr<KafkaActor>>,
    last_post_time: i64,
    last_save_time: i64,
}

impl Default for OutputActor {
    fn default() -> Self {
        Self {
            output_service: Box::new(OutputService::default()),
            twitter_actor: None,
            bluesky_actor: None,
            mastodon_actor: None,
            kafka_actor: None,
            last_post_time: 0,
            last_save_time: 0,
        }
    }
}

impl Actor for OutputActor {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        info!("OutputActor started");
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!("OutputActor stopped");
    }
}

impl OutputActor {
    pub fn new(output_service: Box<dyn OutputServiceTrait>) -> Self {
        Self {
            output_service,
            twitter_actor: None,
            bluesky_actor: None,
            mastodon_actor: None,
            kafka_actor: None,
            last_post_time: 0,
            last_save_time: 0,
        }
    }

    pub fn with_services(
        output_service: Box<dyn OutputServiceTrait>,
        twitter_actor: Option<Addr<TwitterActor>>,
        bluesky_actor: Option<Addr<BlueskyActor>>,
        mastodon_actor: Option<Addr<MastodonActor>>,
        kafka_actor: Option<Addr<KafkaActor>>,
    ) -> Self {
        Self {
            output_service,
            twitter_actor,
            bluesky_actor,
            mastodon_actor,
            kafka_actor,
            last_post_time: 0,
            last_save_time: 0,
        }
    }

    fn should_post(
        &self,
        consecutive_detections: u32,
        timestamp: i64,
        max_confidence: u8,
    ) -> bool {
        let has_enough_detections =
            consecutive_detections >= CONFIG.detection.minimum_detection_frames;
        let enough_time_passed =
            timestamp - self.last_post_time >= CONFIG.output.post_interval;
        let confidence_high_enough = max_confidence >= CONFIG.detection.minimum_post_confidence;

        if has_enough_detections && enough_time_passed && !confidence_high_enough {
            info!(
                "Skipping post: max confidence {} < minimum required {}",
                max_confidence, CONFIG.detection.minimum_post_confidence
            );
        }

        has_enough_detections && enough_time_passed && confidence_high_enough
    }

    fn should_save_image(&self, timestamp: i64) -> bool {
        timestamp - self.last_save_time >= CONFIG.output.image_save_interval
    }
}

impl Handler<PostToSocialMedia> for OutputActor {
    type Result = ResponseActFuture<Self, Result<(), NorppaliveError>>;

    fn handle(&mut self, msg: PostToSocialMedia, _ctx: &mut Self::Context) -> Self::Result {
        info!(
            "Posting to social media with {} detections",
            msg.detections.len()
        );

        let image = msg.image;
        let fut = self.output_service.post_to_social_media(image);
        Box::pin(fut.into_actor(self))
    }
}

impl Handler<SaveDetectionImage> for OutputActor {
    type Result = ResponseActFuture<Self, Result<(), NorppaliveError>>;

    fn handle(&mut self, msg: SaveDetectionImage, _ctx: &mut Self::Context) -> Self::Result {
        info!(
            "Saving detection image with {} detections",
            msg.detections.len()
        );

        Box::pin(
            async move {
                use crate::config::CONFIG;
                use chrono::Utc;
                use std::fs;

                // Create output directory if it doesn't exist
                fs::create_dir_all(&CONFIG.output.output_file_folder)?;

                // Draw bounding boxes on the image using the existing utility function
                let image_with_boxes = draw_boxes_on_provided_image(msg.image, &msg.detections)?;

                // Save the image with timestamp
                let timestamp = Utc::now().timestamp();
                let image_path = format!(
                    "{}/detection-{}.jpg",
                    CONFIG.output.output_file_folder, timestamp
                );

                image_with_boxes.save(&image_path)?;

                info!("Detection image saved to: {}", image_path);
                Ok(())
            }
            .into_actor(self),
        )
    }
}

impl Handler<SaveHeatmapVisualization> for OutputActor {
    type Result = ResponseActFuture<Self, Result<(), NorppaliveError>>;

    fn handle(&mut self, msg: SaveHeatmapVisualization, _ctx: &mut Self::Context) -> Self::Result {
        info!("Saving heatmap visualization");

        Box::pin(
            async move {
                use crate::config::CONFIG;
                use chrono::Utc;
                use image::RgbImage;
                use std::fs;

                // Create output directory if it doesn't exist
                fs::create_dir_all(&CONFIG.output.output_file_folder)?;

                // Create a base image for the heatmap
                let mut base_image = image::DynamicImage::ImageRgb8(RgbImage::new(
                    msg.temp_map.width,
                    msg.temp_map.height,
                ));

                // Draw the temperature map on the base image
                msg.temp_map.draw(&mut base_image)?;

                // Save the heatmap image with timestamp
                let timestamp = Utc::now().timestamp();
                let heatmap_path = format!(
                    "{}/heatmap-{}.jpg",
                    CONFIG.output.output_file_folder, timestamp
                );

                base_image.save(&heatmap_path)?;

                info!("Heatmap visualization saved to: {}", heatmap_path);
                Ok(())
            }
            .into_actor(self),
        )
    }
}

impl Handler<GetServiceStatus> for OutputActor {
    type Result = Result<ServiceStatus, NorppaliveError>;

    fn handle(&mut self, _msg: GetServiceStatus, _ctx: &mut Self::Context) -> Self::Result {
        // Return a basic status for now
        Ok(ServiceStatus {
            name: "Output".to_string(),
            healthy: true,
            last_post_time: if self.last_post_time > 0 {
                Some(self.last_post_time)
            } else {
                None
            },
            error_count: 0,
            rate_limited: false,
        })
    }
}

impl Handler<DetectionCompleted> for OutputActor {
    type Result = ();

    fn handle(&mut self, msg: DetectionCompleted, _ctx: &mut Self::Context) -> Self::Result {
        let detection_count = msg.detections.len();
        let max_confidence = msg.detections.iter().map(|d| d.conf).max().unwrap_or(0);

        info!(
            "DetectionCompleted received: {} detections (max conf: {}%), {} consecutive, timestamp: {}",
            detection_count, max_confidence, msg.consecutive_detections, msg.timestamp
        );

        // Only process if there are detections
        if msg.detections.is_empty() {
            debug!("No detections to process");
            return;
        }

        // Check if we should save the image
        if self.should_save_image(msg.timestamp) {
            self.last_save_time = msg.timestamp;

            // Load image and draw bounding boxes
            match image::open(&msg.image_path) {
                Ok(image) => {
                    match draw_boxes_on_provided_image(image.clone(), &msg.detections) {
                        Ok(annotated_image) => {
                            // Save the annotated image
                            let save_path = format!(
                                "{}/detection-{}.jpg",
                                CONFIG.output.output_file_folder, msg.timestamp
                            );
                            if let Err(e) =
                                std::fs::create_dir_all(&CONFIG.output.output_file_folder)
                            {
                                warn!("Failed to create output directory: {}", e);
                            }
                            if let Err(e) = annotated_image.save(&save_path) {
                                warn!("Failed to save detection image: {}", e);
                            } else {
                                info!("Detection image saved to: {}", save_path);
                            }
                        }
                        Err(e) => warn!("Failed to draw bounding boxes: {}", e),
                    }
                }
                Err(e) => warn!("Failed to open image {}: {}", msg.image_path, e),
            }
        }

        // Check if we should post to social media (includes confidence check)
        if self.should_post(msg.consecutive_detections, msg.timestamp, max_confidence) {
            self.last_post_time = msg.timestamp;
            info!(
                "Posting to social media: {} detections (max conf: {}%) after {} consecutive frames",
                detection_count, max_confidence, msg.consecutive_detections
            );

            // Create the annotated image path
            let annotated_path = format!(
                "{}/detection-{}.jpg",
                CONFIG.output.output_file_folder, msg.timestamp
            );

            let message = format!(
                "Saimaa ringed seal detected! {} seal(s) spotted. #Norppalive #SaimaaRingedSeal",
                detection_count
            );

            // Send to all connected service actors
            let service_post = ServicePost {
                message: message.clone(),
                image_path: annotated_path,
                service_name: "all".to_string(),
            };

            if let Some(ref twitter) = self.twitter_actor {
                twitter.do_send(service_post.clone());
            }
            if let Some(ref bluesky) = self.bluesky_actor {
                bluesky.do_send(service_post.clone());
            }
            if let Some(ref mastodon) = self.mastodon_actor {
                mastodon.do_send(service_post.clone());
            }
            if let Some(ref kafka) = self.kafka_actor {
                kafka.do_send(service_post);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::detection_utils::DetectionResult;
    #[cfg(any(test, feature = "test-utils"))]
    use crate::utils::output::MockOutputService;
    use crate::utils::temperature_map::TemperatureMap;

    use image::{DynamicImage, RgbImage};

    fn create_test_image() -> DynamicImage {
        // Create a simple test image
        let img = RgbImage::new(100, 100);
        DynamicImage::ImageRgb8(img)
    }

    fn create_test_detection() -> DetectionResult {
        DetectionResult {
            r#box: [10, 10, 50, 50],
            cls: 0,
            cls_name: "seal".to_string(),
            conf: 85,
        }
    }

    #[actix::test]
    async fn test_output_actor_startup() {
        let actor = OutputActor::new(Box::new(MockOutputService::new())).start();

        // Test getting service status
        let status = actor.send(GetServiceStatus).await.unwrap().unwrap();
        assert_eq!(status.name, "Output");
        assert!(status.healthy);
        assert_eq!(status.error_count, 0);
        assert!(!status.rate_limited);
    }

    #[actix::test]
    async fn test_post_to_social_media() {
        let actor = OutputActor::new(Box::new(MockOutputService::new())).start();

        let test_image = create_test_image();
        let test_detection = create_test_detection();

        let result = actor
            .send(PostToSocialMedia {
                detections: vec![test_detection],
                image: test_image,
                message: "Test post".to_string(),
            })
            .await
            .unwrap();

        // Should complete without error (using mock service)
        assert!(result.is_ok());
    }

    #[actix::test]
    async fn test_save_detection_image() {
        let actor = OutputActor::new(Box::new(MockOutputService::new())).start();

        let test_image = create_test_image();
        let test_detection = create_test_detection();

        let result = actor
            .send(SaveDetectionImage {
                detections: vec![test_detection],
                image: test_image,
            })
            .await
            .unwrap();

        // Should complete without error (stub implementation)
        assert!(result.is_ok());
    }

    #[actix::test]
    async fn test_save_heatmap_visualization() {
        let actor = OutputActor::new(Box::new(MockOutputService::new())).start();

        let temp_map = TemperatureMap::new(100, 100);

        let result = actor
            .send(SaveHeatmapVisualization { temp_map })
            .await
            .unwrap();

        // Should complete without error (stub implementation)
        assert!(result.is_ok());
    }

    #[actix::test]
    async fn test_get_service_status() {
        let actor = OutputActor::new(Box::new(MockOutputService::new())).start();

        let status = actor.send(GetServiceStatus).await.unwrap().unwrap();

        // Verify status structure
        assert_eq!(status.name, "Output");
        assert!(status.healthy);
        assert!(status.last_post_time.is_none());
        assert_eq!(status.error_count, 0);
        assert!(!status.rate_limited);
    }

    #[actix::test]
    async fn test_multiple_concurrent_operations() {
        let actor = OutputActor::new(Box::new(MockOutputService::new())).start();

        let test_image1 = create_test_image();
        let test_image2 = create_test_image();
        let test_detection = create_test_detection();
        let temp_map = TemperatureMap::new(50, 50);

        // Send operations concurrently but separately to avoid type conflicts
        let post_result = actor.send(PostToSocialMedia {
            detections: vec![test_detection.clone()],
            image: test_image1,
            message: "Test post 1".to_string(),
        });

        let save_result = actor.send(SaveDetectionImage {
            detections: vec![test_detection],
            image: test_image2,
        });

        let heatmap_result = actor.send(SaveHeatmapVisualization { temp_map });

        // Wait for all operations to complete
        let (post_res, save_res, heatmap_res) =
            futures::future::join3(post_result, save_result, heatmap_result).await;

        // All operations should complete successfully
        assert!(post_res.is_ok());
        assert!(post_res.unwrap().is_ok());

        assert!(save_res.is_ok());
        assert!(save_res.unwrap().is_ok());

        assert!(heatmap_res.is_ok());
        assert!(heatmap_res.unwrap().is_ok());
    }

    #[actix::test]
    async fn test_empty_detections_handling() {
        let actor = OutputActor::new(Box::new(MockOutputService::new())).start();

        let test_image = create_test_image();

        // Test with empty detections
        let result = actor
            .send(PostToSocialMedia {
                detections: vec![], // Empty detections
                image: test_image.clone(),
                message: "Test with no detections".to_string(),
            })
            .await
            .unwrap();

        assert!(result.is_ok());

        // Test save with empty detections
        let result = actor
            .send(SaveDetectionImage {
                detections: vec![], // Empty detections
                image: test_image,
            })
            .await
            .unwrap();

        assert!(result.is_ok());
    }
}
