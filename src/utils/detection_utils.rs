use async_trait::async_trait;
use miette::Result;
use reqwest::{multipart, Client};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json;
use std::time::Duration;
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use tracing::{debug, error, info};

use crate::{config::CONFIG, error::NorppaliveError, utils::temperature_map::TemperatureMap};

const CONFIDENCE_MULTIPLIER: f32 = 100.0;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DetectionResult {
    #[serde(deserialize_with = "deserialize_box")]
    pub r#box: [u32; 4],
    #[serde(deserialize_with = "float_to_u8")]
    pub cls: u8,
    pub cls_name: String,
    #[serde(deserialize_with = "deserialize_conf")]
    pub conf: u8,
}

#[derive(Serialize, Deserialize, Debug)]
struct DetectionApiResponse {
    detections: Vec<DetectionResult>,
    inference_time: f64,
    model_type: String,
    model_version: String,
}

// Trait for abstraction to allow mocking
#[async_trait]
pub trait DetectionServiceTrait: Send {
    async fn do_detection(&self, filename: &str) -> Result<Vec<DetectionResult>, NorppaliveError>;
    fn has_heatmap_hotspots(&self, percentile_threshold: f64) -> bool;
    fn update_temperature_map(&mut self, detections: &[DetectionResult]);
    fn filter_acceptable_detections<'a>(
        &self,
        detection_result: &'a [DetectionResult],
    ) -> Vec<&'a DetectionResult>;
    fn get_temperature_map(&self) -> &TemperatureMap;
}

// Mock implementation for testing
#[cfg(test)]
pub struct MockDetectionService {
    pub should_fail: bool,
    pub mock_detections: Vec<DetectionResult>,
    pub temperature_map: TemperatureMap,
    pub mock_has_hotspots: bool,
}

#[cfg(test)]
impl Default for MockDetectionService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
impl MockDetectionService {
    pub fn new() -> Self {
        Self {
            should_fail: false,
            mock_detections: vec![],
            temperature_map: TemperatureMap::new(1280, 720),
            mock_has_hotspots: false,
        }
    }

    pub fn with_mock_detections(mut self, detections: Vec<DetectionResult>) -> Self {
        self.mock_detections = detections;
        self
    }

    pub fn with_failure(mut self, should_fail: bool) -> Self {
        self.should_fail = should_fail;
        self
    }

    #[allow(dead_code)]
    pub fn with_hotspots(mut self, has_hotspots: bool) -> Self {
        self.mock_has_hotspots = has_hotspots;
        self
    }
}

#[cfg(test)]
#[async_trait]
impl DetectionServiceTrait for MockDetectionService {
    async fn do_detection(&self, _filename: &str) -> Result<Vec<DetectionResult>, NorppaliveError> {
        if self.should_fail {
            return Err(NorppaliveError::Other("Mock detection failure".to_string()));
        }
        Ok(self.mock_detections.clone())
    }

    fn has_heatmap_hotspots(&self, _percentile_threshold: f64) -> bool {
        self.mock_has_hotspots
    }

    fn update_temperature_map(&mut self, _detections: &[DetectionResult]) {
        // Mock does nothing
    }

    fn filter_acceptable_detections<'a>(
        &self,
        detection_result: &'a [DetectionResult],
    ) -> Vec<&'a DetectionResult> {
        detection_result.iter().collect()
    }

    fn get_temperature_map(&self) -> &TemperatureMap {
        &self.temperature_map
    }
}

fn float_to_u8<'de, D>(deserializer: D) -> Result<u8, D::Error>
where
    D: Deserializer<'de>,
{
    let f = f32::deserialize(deserializer)?;
    if f * CONFIDENCE_MULTIPLIER > 255.0 {
        return Err(<D::Error as serde::de::Error>::custom(
            "Confidence value out of range",
        ));
    }
    Ok((f * CONFIDENCE_MULTIPLIER) as u8)
}

fn deserialize_box<'de, D>(deserializer: D) -> Result<[u32; 4], D::Error>
where
    D: Deserializer<'de>,
{
    let vec: Vec<f32> = Vec::deserialize(deserializer)?;
    Ok([vec[0] as u32, vec[1] as u32, vec[2] as u32, vec[3] as u32])
}

fn deserialize_conf<'de, D>(deserializer: D) -> Result<u8, D::Error>
where
    D: Deserializer<'de>,
{
    let f = f32::deserialize(deserializer)?;
    Ok((f * 100.0) as u8)
}

/// DetectionService handles ML detection and temperature map tracking.
/// Output operations (posting, saving) are handled by OutputActor.
pub struct DetectionService {
    temperature_map: TemperatureMap,
}

impl Default for DetectionService {
    fn default() -> Self {
        Self::new()
    }
}

impl DetectionService {
    pub fn new() -> Self {
        Self {
            temperature_map: TemperatureMap::new(1280, 720),
        }
    }

    pub fn has_heatmap_hotspots(&self, percentile_threshold: f64) -> bool {
        self.temperature_map
            .has_histogram_hotspots(percentile_threshold)
    }

    pub fn get_temperature_map(&self) -> &TemperatureMap {
        &self.temperature_map
    }

    async fn do_detection_impl(
        &self,
        filename: &str,
    ) -> Result<Vec<DetectionResult>, NorppaliveError> {
        info!("Starting detection process for file: {}", filename);

        let mut file_handle = File::open(&filename).await?;
        let mut file_bytes = Vec::new();
        file_handle.read_to_end(&mut file_bytes).await?;

        let form = multipart::Form::new().part(
            "file",
            multipart::Part::bytes(file_bytes)
                .file_name(filename.to_string())
                .mime_str("image/png")
                .expect("Failed to create multipart part"),
        );

        info!("File loaded into memory!");
        let client = Client::builder().timeout(Duration::from_secs(10)).build()?;

        info!(
            "Sending request to detection API: {}",
            &CONFIG.detection.api_url
        );

        let res = match client
            .post(&CONFIG.detection.api_url)
            .multipart(form)
            .send()
            .await
        {
            Ok(response) => {
                info!("Received response with status: {}", response.status());
                response
            }
            Err(err) => {
                error!("Failed to send request: {}", err);
                return Err(NorppaliveError::Other(format!(
                    "Could not send the request: {}",
                    err
                )));
            }
        };

        if !res.status().is_success() {
            let status = res.status();
            let error_text = res
                .text()
                .await
                .unwrap_or_else(|_| "Could not read error response".to_string());
            error!(
                "API returned error status: {}, message: {}",
                status, error_text
            );
            return Err(NorppaliveError::Other(format!(
                "API error: {} - {}",
                status, error_text
            )));
        }

        let response_text = match res.text().await {
            Ok(text) => {
                info!("Received raw response: {}", text);
                text
            }
            Err(e) => {
                error!("Failed to get response text: {}", e);
                return Err(NorppaliveError::ReqwestError(e));
            }
        };

        info!("Parsing JSON response");
        let api_response = match serde_json::from_str::<DetectionApiResponse>(&response_text) {
            Ok(response) => {
                info!(
                    "Successfully parsed API response. Inference time: {}, Model: {} {}",
                    response.inference_time, response.model_type, response.model_version
                );
                response
            }
            Err(err) => {
                error!("Failed to parse JSON response: {}", err);
                return Err(NorppaliveError::JsonError(err));
            }
        };

        let result = api_response.detections;
        info!("Found {} detection results", result.len());
        debug!("Detection results: {:?}", result);

        Ok(result)
    }

    /// Filters detections based on confidence threshold and ignore points
    pub fn filter_acceptable_detections<'a>(
        &self,
        detection_result: &'a [DetectionResult],
    ) -> Vec<&'a DetectionResult> {
        fn point_inside_box(
            x: u32,
            y: u32,
            box_x1: u32,
            box_y1: u32,
            box_x2: u32,
            box_y2: u32,
        ) -> bool {
            x >= box_x1 && x <= box_x2 && y >= box_y1 && y <= box_y2
        }
        detection_result
            .iter()
            .filter(|detection| detection.conf > CONFIG.detection.minimum_detection_percentage)
            .filter(|detection| {
                !CONFIG.detection.ignore_points.iter().any(|ignore_point| {
                    let x = ignore_point.x;
                    let y = ignore_point.y;
                    let box_x1 = detection.r#box[0];
                    let box_y1 = detection.r#box[1];
                    let box_x2 = detection.r#box[2];
                    let box_y2 = detection.r#box[3];
                    point_inside_box(x, y, box_x1, box_y1, box_x2, box_y2)
                })
            })
            .collect()
    }
}

#[async_trait]
impl DetectionServiceTrait for DetectionService {
    async fn do_detection(&self, filename: &str) -> Result<Vec<DetectionResult>, NorppaliveError> {
        self.do_detection_impl(filename).await
    }

    fn has_heatmap_hotspots(&self, percentile_threshold: f64) -> bool {
        self.temperature_map
            .has_histogram_hotspots(percentile_threshold)
    }

    fn update_temperature_map(&mut self, detections: &[DetectionResult]) {
        self.temperature_map
            .apply_decay(CONFIG.detection.heatmap_decay_rate);

        let acceptable = self.filter_acceptable_detections(detections);
        if !acceptable.is_empty() {
            self.temperature_map.set_points_from_detections(&acceptable);
        }
    }

    fn filter_acceptable_detections<'a>(
        &self,
        detection_result: &'a [DetectionResult],
    ) -> Vec<&'a DetectionResult> {
        DetectionService::filter_acceptable_detections(self, detection_result)
    }

    fn get_temperature_map(&self) -> &TemperatureMap {
        &self.temperature_map
    }
}
