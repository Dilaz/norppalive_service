use actix::prelude::*;
use async_trait::async_trait;

use crate::actors::{DetectionActor, OutputActor, SupervisorActor};
use crate::error::NorppaliveError;
use crate::messages::{
    DetectionStats, GetDetectionStats, GetServiceStatus, GetSystemHealth, PostToSocialMedia,
    SaveDetectionImage, SaveHeatmapVisualization, ServiceStatus, SystemHealth,
};
use crate::utils::temperature_map::TemperatureMap;

use super::trait_def::{MessageBus, PostRequest, SaveImageRequest};

/// Actor-based implementation of MessageBus that communicates with local actors
pub struct ActorMessageBus {
    output_actor: Addr<OutputActor>,
    detection_actor: Addr<DetectionActor>,
    supervisor_actor: Addr<SupervisorActor>,
}

impl ActorMessageBus {
    pub fn new(
        output_actor: Addr<OutputActor>,
        detection_actor: Addr<DetectionActor>,
        supervisor_actor: Addr<SupervisorActor>,
    ) -> Self {
        Self {
            output_actor,
            detection_actor,
            supervisor_actor,
        }
    }
}

#[async_trait]
impl MessageBus for ActorMessageBus {
    async fn post_to_social_media(&self, request: PostRequest) -> Result<(), NorppaliveError> {
        let msg = PostToSocialMedia {
            detections: request.detections,
            image: request.image,
            message: request.message,
        };

        self.output_actor
            .send(msg)
            .await
            .map_err(|e| NorppaliveError::Other(format!("Actor communication error: {}", e)))?
    }

    async fn save_detection_image(&self, request: SaveImageRequest) -> Result<(), NorppaliveError> {
        let msg = SaveDetectionImage {
            detections: request.detections,
            image: request.image,
        };

        self.output_actor
            .send(msg)
            .await
            .map_err(|e| NorppaliveError::Other(format!("Actor communication error: {}", e)))?
    }

    async fn save_heatmap_visualization(
        &self,
        temp_map: TemperatureMap,
    ) -> Result<(), NorppaliveError> {
        let msg = SaveHeatmapVisualization { temp_map };

        self.output_actor
            .send(msg)
            .await
            .map_err(|e| NorppaliveError::Other(format!("Actor communication error: {}", e)))?
    }

    async fn get_detection_stats(&self) -> Result<DetectionStats, NorppaliveError> {
        self.detection_actor
            .send(GetDetectionStats)
            .await
            .map_err(|e| NorppaliveError::Other(format!("Actor communication error: {}", e)))?
    }

    async fn get_system_health(&self) -> Result<SystemHealth, NorppaliveError> {
        self.supervisor_actor
            .send(GetSystemHealth)
            .await
            .map_err(|e| NorppaliveError::Other(format!("Actor communication error: {}", e)))?
    }

    async fn get_service_statuses(&self) -> Result<Vec<ServiceStatus>, NorppaliveError> {
        // For now, we'll get this from the output actor
        // In a more complex setup, we might query each service actor individually
        let status = self
            .output_actor
            .send(GetServiceStatus)
            .await
            .map_err(|e| NorppaliveError::Other(format!("Actor communication error: {}", e)))??;
        Ok(vec![status]) // Convert single status to vector for now
    }
}
