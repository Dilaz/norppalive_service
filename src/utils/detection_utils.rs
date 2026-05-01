use async_trait::async_trait;
use miette::Result;
use reqwest::{multipart, Client};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json;
use std::time::Duration;
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use tracing::{debug, error, info};

use crate::{config::CONFIG, error::NorppaliveError};

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

/// Wire shape returned by norppalive-api `POST /detect` (bare JSON array).
/// Translated into `DetectionResult` at the HTTP boundary so the rest of the
/// pipeline keeps its u32 box / 0-100 confidence representation.
#[derive(Deserialize, Debug, Clone)]
struct ApiDetection {
    bbox: [f32; 4],
    class_id: u32,
    label: String,
    confidence: f32,
}

impl From<ApiDetection> for DetectionResult {
    fn from(d: ApiDetection) -> Self {
        DetectionResult {
            r#box: [
                d.bbox[0] as u32,
                d.bbox[1] as u32,
                d.bbox[2] as u32,
                d.bbox[3] as u32,
            ],
            cls: u8::try_from(d.class_id).unwrap_or(u8::MAX),
            cls_name: d.label,
            conf: (d.confidence * CONFIDENCE_MULTIPLIER).clamp(0.0, 255.0) as u8,
        }
    }
}

// Trait for abstraction to allow mocking
#[async_trait]
pub trait DetectionServiceTrait: Send {
    async fn do_detection(&self, filename: &str) -> Result<Vec<DetectionResult>, NorppaliveError>;
    fn filter_acceptable_detections<'a>(
        &self,
        detection_result: &'a [DetectionResult],
    ) -> Vec<&'a DetectionResult>;
}

// Mock implementation for testing
#[cfg(test)]
pub struct MockDetectionService {
    pub should_fail: bool,
    pub mock_detections: Vec<DetectionResult>,
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

    fn filter_acceptable_detections<'a>(
        &self,
        detection_result: &'a [DetectionResult],
    ) -> Vec<&'a DetectionResult> {
        detection_result.iter().collect()
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

/// DetectionService handles ML detection.
/// Output operations (posting, saving) are handled by OutputActor.
#[derive(Default)]
pub struct DetectionService;

impl DetectionService {
    pub fn new() -> Self {
        Self
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
            "image",
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
                info!(bytes = text.len(), "Received raw response: {:?}", text);
                text
            }
            Err(e) => {
                error!("Failed to get response text: {}", e);
                return Err(e.into());
            }
        };

        info!("Parsing JSON response");
        let api_detections: Vec<ApiDetection> = match serde_json::from_str(&response_text) {
            Ok(detections) => detections,
            Err(err) => {
                error!("Failed to parse JSON response: {}", err);
                return Err(err.into());
            }
        };

        let result: Vec<DetectionResult> = api_detections
            .into_iter()
            .map(DetectionResult::from)
            .collect();
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

    fn filter_acceptable_detections<'a>(
        &self,
        detection_result: &'a [DetectionResult],
    ) -> Vec<&'a DetectionResult> {
        DetectionService::filter_acceptable_detections(self, detection_result)
    }
}
