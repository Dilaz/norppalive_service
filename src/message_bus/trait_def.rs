use async_trait::async_trait;
use image::DynamicImage;

use crate::error::NorppaliveError;
use crate::messages::{DetectionStats, ServiceStatus, SystemHealth};
use crate::utils::detection_utils::DetectionResult;
use crate::utils::temperature_map::TemperatureMap;

/// Abstraction layer for communication with detection and output services.
/// This allows the same interface whether services are running as actors
/// in the same process or as separate services accessible via HTTP.
#[async_trait]
pub trait MessageBus: Send + Sync {
    /// Trigger a social media post with detection results
    async fn post_to_social_media(&self, request: PostRequest) -> Result<(), NorppaliveError>;

    /// Save detection image to filesystem
    async fn save_detection_image(&self, request: SaveImageRequest) -> Result<(), NorppaliveError>;

    /// Save heatmap visualization
    async fn save_heatmap_visualization(
        &self,
        temp_map: TemperatureMap,
    ) -> Result<(), NorppaliveError>;

    /// Get current detection statistics
    async fn get_detection_stats(&self) -> Result<DetectionStats, NorppaliveError>;

    /// Get system health status
    async fn get_system_health(&self) -> Result<SystemHealth, NorppaliveError>;

    /// Get status of individual services
    async fn get_service_statuses(&self) -> Result<Vec<ServiceStatus>, NorppaliveError>;
}

/// Request to post to social media
#[derive(Debug, Clone)]
pub struct PostRequest {
    pub detections: Vec<DetectionResult>,
    pub image: DynamicImage,
    pub message: String,
}

/// Request to save detection image
#[derive(Debug, Clone)]
pub struct SaveImageRequest {
    pub detections: Vec<DetectionResult>,
    pub image: DynamicImage,
}
