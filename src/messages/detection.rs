use actix::prelude::*;
use image::DynamicImage;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::utils::detection_utils::DetectionResult;

/// Messages for DetectionActor

#[derive(Message)]
#[rtype(result = "Result<Vec<DetectionResult>, crate::error::NorppaliveError>")]
pub struct ProcessFrame {
    /// The image data in memory
    pub image_data: Arc<DynamicImage>,
    /// Path where the image was saved (for detection API that needs file path)
    pub image_path: String,
    pub timestamp: i64,
    pub reply_to: Addr<crate::actors::StreamActor>,
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct DetectionCompleted {
    pub detections: Vec<DetectionResult>,
    pub timestamp: i64,
    pub consecutive_detections: u32,
    /// The original image data in memory (avoids re-reading from disk)
    pub image_data: Arc<DynamicImage>,
}

#[derive(Message)]
#[rtype(result = "Result<DetectionStats, crate::error::NorppaliveError>")]
pub struct GetDetectionStats;

/// Message to signal that the DetectionActor is ready for the next frame
#[derive(Message)]
#[rtype(result = "()")]
pub struct ReadyForNextFrame;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectionStats {
    pub total_detections: u64,
    pub consecutive_detections: u32,
    pub last_detection_time: i64,
    pub temperature_map_hotspots: bool,
}
