use async_trait::async_trait;
use reqwest::Client;

use crate::error::NorppaliveError;
use crate::messages::{DetectionStats, ServiceStatus, SystemHealth};
use crate::utils::temperature_map::TemperatureMap;

use super::trait_def::{MessageBus, PostRequest, SaveImageRequest};

/// HTTP-based implementation of MessageBus for communicating with external services
/// This will be used when the services run as separate binaries
pub struct HttpMessageBus {
    #[allow(dead_code)]
    base_url: String,
    #[allow(dead_code)]
    client: Client,
}

impl HttpMessageBus {
    pub fn new(base_url: String) -> Self {
        Self {
            base_url,
            client: Client::new(),
        }
    }
}

#[async_trait]
impl MessageBus for HttpMessageBus {
    async fn post_to_social_media(&self, _request: PostRequest) -> Result<(), NorppaliveError> {
        // Future implementation: POST to /api/v1/post
        todo!("HTTP implementation not yet available - use ActorMessageBus for now")
    }

    async fn save_detection_image(
        &self,
        _request: SaveImageRequest,
    ) -> Result<(), NorppaliveError> {
        // Future implementation: POST to /api/v1/save-image
        todo!("HTTP implementation not yet available - use ActorMessageBus for now")
    }

    async fn save_heatmap_visualization(
        &self,
        _temp_map: TemperatureMap,
    ) -> Result<(), NorppaliveError> {
        // Future implementation: POST to /api/v1/save-heatmap
        todo!("HTTP implementation not yet available - use ActorMessageBus for now")
    }

    async fn get_detection_stats(&self) -> Result<DetectionStats, NorppaliveError> {
        // Future implementation: GET from /api/v1/stats
        todo!("HTTP implementation not yet available - use ActorMessageBus for now")
    }

    async fn get_system_health(&self) -> Result<SystemHealth, NorppaliveError> {
        // Future implementation: GET from /api/v1/health
        todo!("HTTP implementation not yet available - use ActorMessageBus for now")
    }

    async fn get_service_statuses(&self) -> Result<Vec<ServiceStatus>, NorppaliveError> {
        // Future implementation: GET from /api/v1/services/status
        todo!("HTTP implementation not yet available - use ActorMessageBus for now")
    }
}
