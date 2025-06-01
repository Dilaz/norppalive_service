use actix::prelude::*;
use tracing::info;

use crate::error::NorppaliveError;
use crate::messages::{
    GetServiceStatus, PostToSocialMedia, SaveDetectionImage, SaveHeatmapVisualization,
    ServiceStatus,
};
use crate::utils::output::OutputService;

#[cfg(test)]
use crate::utils::output::{MockOutputService, OutputServiceTrait};

/// OutputActor coordinates output operations (posting, saving)
#[derive(Default)]
pub struct OutputActor {
    output_service: OutputService,
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
    pub fn new() -> Self {
        Self::default()
    }

    #[cfg(test)]
    pub fn new_with_mocks() -> Self {
        // In tests, we don't want to use real services
        Self {
            output_service: OutputService::default(), // We'll override the methods in tests
        }
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

        #[cfg(test)]
        {
            // In tests, use mock service to avoid real posts
            Box::pin(
                async move {
                    let mock_service = MockOutputService::new();
                    mock_service.post_to_social_media(image).await
                }
                .into_actor(self),
            )
        }
        #[cfg(not(test))]
        {
            // In production, use the stored service
            let output_service = self.output_service.clone();
            Box::pin(
                async move { output_service.post_to_social_media(image).await }.into_actor(self),
            )
        }
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
                // TODO: Implement image saving logic
                // For now, just return success
                Ok(())
            }
            .into_actor(self),
        )
    }
}

impl Handler<SaveHeatmapVisualization> for OutputActor {
    type Result = ResponseActFuture<Self, Result<(), NorppaliveError>>;

    fn handle(&mut self, _msg: SaveHeatmapVisualization, _ctx: &mut Self::Context) -> Self::Result {
        info!("Saving heatmap visualization");

        Box::pin(
            async move {
                // TODO: Implement heatmap saving logic
                // For now, just return success
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
            last_post_time: None,
            error_count: 0,
            rate_limited: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::detection_utils::DetectionResult;
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
        let actor = OutputActor::new_with_mocks().start();

        // Test getting service status
        let status = actor.send(GetServiceStatus).await.unwrap().unwrap();
        assert_eq!(status.name, "Output");
        assert!(status.healthy);
        assert_eq!(status.error_count, 0);
        assert!(!status.rate_limited);
    }

    #[actix::test]
    async fn test_post_to_social_media() {
        let actor = OutputActor::new_with_mocks().start();

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
        let actor = OutputActor::new_with_mocks().start();

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
        let actor = OutputActor::new_with_mocks().start();

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
        let actor = OutputActor::new_with_mocks().start();

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
        let actor = OutputActor::new_with_mocks().start();

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
        let actor = OutputActor::new_with_mocks().start();

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
