use actix::prelude::*;
use image::DynamicImage;
use serde::{Deserialize, Serialize};

use crate::utils::detection_utils::DetectionResult;
use crate::utils::temperature_map::TemperatureMap;

/// Messages for OutputActor and Service Actors

#[derive(Message)]
#[rtype(result = "Result<(), crate::error::NorppaliveError>")]
pub struct PostToSocialMedia {
    pub detections: Vec<DetectionResult>,
    pub image: DynamicImage,
    pub message: String,
}

#[derive(Message)]
#[rtype(result = "Result<(), crate::error::NorppaliveError>")]
pub struct SaveDetectionImage {
    pub detections: Vec<DetectionResult>,
    pub image: DynamicImage,
}

#[derive(Message)]
#[rtype(result = "Result<(), crate::error::NorppaliveError>")]
pub struct SaveHeatmapVisualization {
    pub temp_map: TemperatureMap,
}

/// Message for individual service actors
#[derive(Message, Clone)]
#[rtype(result = "Result<ServicePostResult, crate::error::NorppaliveError>")]
pub struct ServicePost {
    pub message: String,
    pub image_path: String,
    pub service_name: String,
}

#[derive(Message)]
#[rtype(result = "Result<ServiceStatus, crate::error::NorppaliveError>")]
pub struct GetServiceStatus;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceStatus {
    pub name: String,
    pub healthy: bool,
    pub last_post_time: Option<i64>,
    pub error_count: u32,
    pub rate_limited: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServicePostResult {
    pub success: bool,
    pub service_name: String,
    pub error_message: Option<String>,
    pub posted_at: i64,
}
