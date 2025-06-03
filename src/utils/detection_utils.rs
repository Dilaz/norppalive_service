use async_trait::async_trait;
use chrono::Local;
use image;
use image::DynamicImage;
use lazy_static::lazy_static;
use miette::Result;
use reqwest::{multipart, Client};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json;
use std::time::Duration;
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use tracing::{debug, error, info};

use crate::{
    config::CONFIG, error::NorppaliveError, services::DetectionKafkaService,
    utils::image_utils::draw_boxes_on_image, utils::temperature_map::TemperatureMap,
};

use super::output::OutputService;

// For now, we'll need to access CONFIG through a function or pass it as a parameter
// This is a temporary solution until we refactor to use dependency injection
use std::sync::atomic::{AtomicBool, Ordering};

lazy_static! {
    static ref SHUTDOWN: AtomicBool = AtomicBool::new(false);
}

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
pub trait DetectionServiceTrait {
    async fn do_detection(&self, filename: &str) -> Result<Vec<DetectionResult>, NorppaliveError>;
    async fn process_detection(
        &mut self,
        detection_result: &[DetectionResult],
        detections_in_row: u32,
    ) -> Option<u32>;
    fn has_heatmap_hotspots(&self, percentile_threshold: f64) -> bool;
}

// Mock implementation for testing
#[cfg(test)]
pub struct MockDetectionService {
    pub should_fail: bool,
    pub mock_detections: Vec<DetectionResult>,
    pub last_post_time: i64,
    pub last_image_save_time: i64,
    pub last_detection_time: i64,
    pub last_heatmap_save_time: i64,
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
            last_post_time: 0,
            last_image_save_time: 0,
            last_detection_time: 0,
            last_heatmap_save_time: 0,
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

    async fn process_detection(
        &mut self,
        detection_result: &[DetectionResult],
        detections_in_row: u32,
    ) -> Option<u32> {
        if detection_result.is_empty() {
            Some(0)
        } else {
            Some(detections_in_row + 1)
        }
    }

    fn has_heatmap_hotspots(&self, _percentile_threshold: f64) -> bool {
        self.mock_has_hotspots
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

pub struct DetectionService {
    output_service: OutputService,
    detection_kafka_service: DetectionKafkaService,
    last_post_time: i64,
    last_image_save_time: i64,
    last_detection_time: i64,
    last_heatmap_save_time: i64,
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
            output_service: OutputService::default(),
            detection_kafka_service: DetectionKafkaService::default(),
            last_post_time: 0,
            last_image_save_time: 0,
            last_detection_time: 0,
            last_heatmap_save_time: 0,
            temperature_map: TemperatureMap::new(1280, 720),
        }
    }

    pub fn has_heatmap_hotspots(&self, percentile_threshold: f64) -> bool {
        self.temperature_map
            .has_histogram_hotspots(percentile_threshold)
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
                SHUTDOWN.store(true, Ordering::Relaxed);
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

    async fn process_detection_impl(
        &mut self,
        detection_result: &[DetectionResult],
        detections_in_row: u32,
    ) -> Option<u32> {
        self.temperature_map
            .apply_decay(CONFIG.detection.heatmap_decay_rate);

        if detection_result.is_empty() {
            debug!("No detections");
            info!("Creating and saving heatmap visualization (no detections)");
            self.save_heatmap_visualization(&self.temperature_map);
            return Some(0);
        }

        let save_image_candidates: Vec<&DetectionResult> = detection_result
            .iter()
            .filter(|d| {
                d.conf >= CONFIG.detection.save_image_confidence
                    && d.conf < CONFIG.detection.minimum_detection_percentage
            })
            .collect();
        if !save_image_candidates.is_empty() {
            let time_condition = self.last_image_save_time == 0
                || (self.last_image_save_time + CONFIG.output.image_save_interval * 60)
                    < Local::now().timestamp();
            if time_condition {
                info!("Saving image due to save_image_confidence threshold");
                self.save_detection_image(&save_image_candidates).await;
            }
        }

        info!("Found {} seals", detection_result.len());
        let acceptable_detections = self.filter_acceptable_detections(detection_result);

        if acceptable_detections.is_empty() {
            debug!("No acceptable detections");
            info!("Creating and saving heatmap visualization (no acceptable detections)");
            self.save_heatmap_visualization(&self.temperature_map);
            return Some(0);
        }

        self.temperature_map
            .set_points_from_detections(&acceptable_detections);
        info!(
            "Found {} acceptable detections with confidence threshold of {}",
            acceptable_detections.len(),
            CONFIG.detection.minimum_detection_percentage
        );
        self.last_detection_time = Local::now().timestamp();
        info!("Creating and saving heatmap visualization");
        self.save_heatmap_visualization(&self.temperature_map);
        self.last_heatmap_save_time = Local::now().timestamp();
        self.process_alerts(&acceptable_detections, detections_in_row)
            .await;
        Some(detections_in_row + 1)
    }

    fn filter_acceptable_detections<'a>(
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

    async fn process_alerts(
        &mut self,
        acceptable_detections: &[&DetectionResult],
        detections_in_row: u32,
    ) {
        if self.should_post(detections_in_row) {
            self.post_to_social_media(acceptable_detections).await;
        } else if self.should_save_image() {
            self.save_detection_image(acceptable_detections).await;
        }
    }

    async fn post_to_social_media(&mut self, acceptable_detections: &[&DetectionResult]) {
        info!("Posting to social media based on heatmap analysis");
        let image_result = draw_boxes_on_image(acceptable_detections);
        if let Ok(image) = image_result {
            info!("Saving image for social media post (ignoring save interval)");
            self.save_detection_image(acceptable_detections).await;
            if let Err(err) = self
                .output_service
                .post_to_social_media(image.clone())
                .await
            {
                error!("Could not post to social media: {}", err);
            } else {
                self.last_post_time = Local::now().timestamp();
                info!("Successfully posted to social media");
            }
        } else {
            error!("Could not create image for social media");
        }
    }

    async fn save_detection_image(&mut self, acceptable_detections: &[&DetectionResult]) {
        info!("Saving image to a file based on heatmap detection!");
        let image_result = draw_boxes_on_image(acceptable_detections);
        if let Ok(image_buffer) = image_result {
            let timestamp = Local::now().format("%Y%m%d_%H%M%S").to_string();
            let output_folder_path = std::path::PathBuf::from(&CONFIG.output.output_file_folder);
            let filename = format!(
                "{}",
                output_folder_path
                    .join(format!("detection_{}.jpg", timestamp))
                    .display()
            );
            if let Err(e) = image_buffer.save(&filename) {
                error!("Failed to save image to file {}: {}", filename, e);
            } else {
                info!("Image saved to file: {}", filename);
                self.last_image_save_time = Local::now().timestamp();
                let owned_detections: Vec<DetectionResult> =
                    acceptable_detections.iter().map(|&d| d.clone()).collect();
                if let Err(e) = self
                    .detection_kafka_service
                    .send_detection_notification(&owned_detections, &filename, "detection")
                    .await
                {
                    error!("Failed to send detection notification to Kafka: {}", e);
                }
            }
        } else {
            error!("Could not create image for saving");
        }
    }

    fn save_heatmap_visualization(&self, temp_map: &TemperatureMap) {
        info!("Saving heatmap visualization");
        let mut heatmap_image = DynamicImage::new_rgba8(temp_map.width, temp_map.height);

        if let Err(e) = temp_map.draw(&mut heatmap_image) {
            error!("Failed to draw heatmap: {}", e);
            return;
        }

        let output_folder_path = std::path::PathBuf::from(&CONFIG.output.output_file_folder);
        let filename = format!("{}", output_folder_path.join("heatmap.jpg").display());
        let heatmap_image_rgb = heatmap_image.to_rgb8();
        if let Err(e) = heatmap_image_rgb.save(&filename) {
            error!(
                "Failed to save heatmap visualization to {}: {}",
                filename, e
            );
        } else {
            info!("Heatmap visualization saved to {}", filename);
        }
    }

    fn should_post(&self, detections_in_row: u32) -> bool {
        let time_condition = self.last_post_time == 0
            || (self.last_post_time + CONFIG.output.post_interval * 60) < Local::now().timestamp();

        let consecutive_condition = detections_in_row >= CONFIG.detection.minimum_detection_frames;
        let hotspot_condition = self
            .temperature_map
            .has_histogram_hotspots(CONFIG.detection.heatmap_threshold.into());

        if hotspot_condition && time_condition {
            info!("Should post: Hotspot detected and time condition met.");
            debug!(
                "Hotspot: {}, Time: {}, Last Post: {}, Interval: {}",
                hotspot_condition,
                time_condition,
                self.last_post_time,
                CONFIG.output.post_interval * 60
            );
            return true;
        }
        if consecutive_condition && time_condition {
            info!("Should post: Consecutive detections and time condition met.");
            debug!(
                "Consecutive: {}, Detections in row: {}, Time: {}, Last Post: {}, Interval: {}",
                consecutive_condition,
                detections_in_row,
                time_condition,
                self.last_post_time,
                CONFIG.output.post_interval * 60
            );
            return true;
        }
        debug!(
            "Should not post. Hotspot: {}, Consecutive: {}, Time: {}, Last Post: {}, Interval: {}",
            hotspot_condition,
            consecutive_condition,
            time_condition,
            self.last_post_time,
            CONFIG.output.post_interval * 60
        );
        false
    }

    fn should_save_image(&self) -> bool {
        let time_condition = self.last_image_save_time == 0
            || (self.last_image_save_time + CONFIG.output.image_save_interval * 60)
                < Local::now().timestamp();

        let hotspot_condition = self
            .temperature_map
            .has_histogram_hotspots(CONFIG.detection.heatmap_threshold.into());

        if hotspot_condition && time_condition {
            info!("Should save image: Hotspot detected and time condition met for image saving.");
            debug!(
                "Hotspot: {}, Time: {}, Last Save: {}, Interval: {}",
                hotspot_condition,
                time_condition,
                self.last_image_save_time,
                CONFIG.output.image_save_interval * 60
            );
            return true;
        }
        debug!(
            "Should not save image. Hotspot: {}, Time: {}, Last Save: {}, Interval: {}",
            hotspot_condition,
            time_condition,
            self.last_image_save_time,
            CONFIG.output.image_save_interval * 60
        );
        false
    }
}

#[async_trait]
impl DetectionServiceTrait for DetectionService {
    async fn do_detection(&self, filename: &str) -> Result<Vec<DetectionResult>, NorppaliveError> {
        self.do_detection_impl(filename).await
    }

    async fn process_detection(
        &mut self,
        detection_result: &[DetectionResult],
        detections_in_row: u32,
    ) -> Option<u32> {
        self.process_detection_impl(detection_result, detections_in_row)
            .await
    }

    fn has_heatmap_hotspots(&self, percentile_threshold: f64) -> bool {
        self.temperature_map
            .has_histogram_hotspots(percentile_threshold)
    }
}
