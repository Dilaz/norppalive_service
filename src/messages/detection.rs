use actix::prelude::*;
use serde::{Deserialize, Serialize};

use crate::utils::detection_utils::DetectionResult;

/// Messages for DetectionActor

#[derive(Message)]
#[rtype(result = "Result<Vec<DetectionResult>, crate::error::NorppaliveError>")]
pub struct ProcessFrame {
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
