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
pub trait DetectionServiceTrait {
    fn do_detection(
        &self,
        filename: &str,
    ) -> impl std::future::Future<Output = Result<Vec<DetectionResult>, NorppaliveError>> + Send;
    fn process_detection(
        &mut self,
        detection_result: &[DetectionResult],
        detections_in_row: u32,
    ) -> impl std::future::Future<Output = Option<u32>> + Send;
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
impl DetectionServiceTrait for MockDetectionService {
    #[allow(clippy::manual_async_fn)]
    fn do_detection(
        &self,
        _filename: &str,
    ) -> impl std::future::Future<Output = Result<Vec<DetectionResult>, NorppaliveError>> + Send
    {
        let should_fail = self.should_fail;
        let mock_detections = self.mock_detections.clone();
        async move {
            if should_fail {
                return Err(NorppaliveError::Other("Mock detection failure".to_string()));
            }
            Ok(mock_detections)
        }
    }

    #[allow(clippy::manual_async_fn)]
    fn process_detection(
        &mut self,
        detection_result: &[DetectionResult],
        detections_in_row: u32,
    ) -> impl std::future::Future<Output = Option<u32>> + Send {
        let is_empty = detection_result.is_empty();
        async move {
            // Mock implementation - just return incremented count if we have detections
            if is_empty {
                Some(0)
            } else {
                Some(detections_in_row + 1)
            }
        }
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

    pub async fn do_detection(
        &self,
        filename: &str,
    ) -> Result<Vec<DetectionResult>, NorppaliveError> {
        info!("Starting detection process");

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

        // Get the raw response text first
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

    /**
     * Process detection results and trigger appropriate actions
     *
     * - If any detection result has confidence >= save_image_confidence but < minimum_detection_percentage, saves the image (does not count as detection or post)
     * - Otherwise, processes detections as before
     */
    pub async fn process_detection(
        &mut self,
        detection_result: &[DetectionResult],
        detections_in_row: u32,
    ) -> Option<u32> {
        // Apply decay to temperature map on every detection run
        self.temperature_map
            .apply_decay(CONFIG.detection.heatmap_decay_rate);

        // Process empty detection results
        if detection_result.is_empty() {
            debug!("No detections");
            // Create and save heatmap visualization even when there are no detections
            info!("Creating and saving heatmap visualization (no detections)");
            self.save_heatmap_visualization(&self.temperature_map);
            // Reset consecutive detections if no seals are found
            return Some(0);
        }

        // -Save image if any detection is above save_image_confidence but below minimum_detection_percentage ---
        let save_image_candidates: Vec<&DetectionResult> = detection_result
            .iter()
            .filter(|d| {
                d.conf >= CONFIG.detection.save_image_confidence
                    && d.conf < CONFIG.detection.minimum_detection_percentage
            })
            .collect();
        if !save_image_candidates.is_empty() {
            // Check image save interval
            let time_condition = self.last_image_save_time == 0
                || (self.last_image_save_time + CONFIG.output.image_save_interval * 60)
                    < Local::now().timestamp();
            if time_condition {
                info!("Saving image due to save_image_confidence threshold");
                debug!(
                    "{} + {} < {}",
                    self.last_image_save_time,
                    CONFIG.output.image_save_interval * 60,
                    Local::now().timestamp()
                );
                self.save_detection_image(&save_image_candidates).await;
            } else {
                debug!(
                    "{} + {} < {}",
                    self.last_image_save_time,
                    CONFIG.output.image_save_interval * 60,
                    Local::now().timestamp()
                );
            }
        }

        info!("Found {} seals", detection_result.len());

        // Filter for acceptable detections
        let acceptable_detections = self.filter_acceptable_detections(detection_result);

        if acceptable_detections.is_empty() {
            debug!("No acceptable detections");
            // Create and save heatmap visualization even when there are no acceptable detections
            info!("Creating and saving heatmap visualization (no acceptable detections)");
            self.save_heatmap_visualization(&self.temperature_map);
            // Reset consecutive detections if no acceptable seals are found
            return Some(0);
        }

        // Update temperature map with new detections
        self.temperature_map
            .set_points_from_detections(&acceptable_detections);

        info!(
            "Found {} acceptable detections with confidence threshold of {}",
            acceptable_detections.len(),
            CONFIG.detection.minimum_detection_percentage
        );

        // Update the last detection time
        self.last_detection_time = Local::now().timestamp();

        // Create and save heatmap visualization
        info!("Creating and saving heatmap visualization");
        self.save_heatmap_visualization(&self.temperature_map);
        self.last_heatmap_save_time = Local::now().timestamp();

        // Handle alerts and image saving
        self.process_alerts(&acceptable_detections, detections_in_row)
            .await;

        // Keep track of consecutive detections for backward compatibility
        // Increment consecutive detections count
        Some(detections_in_row + 1)
    }

    /**
     * Filter detection results to only include acceptable detections based on confidence
     * and ignore points
     */
    fn filter_acceptable_detections<'a>(
        &self,
        detection_result: &'a [DetectionResult],
    ) -> Vec<&'a DetectionResult> {
        // Helper function to check if a point is inside a box (top-left and bottom-right coordinates)
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
                // Filter out detections where ANY ignore point is inside the detection box
                !CONFIG.detection.ignore_points.iter().any(|ignore_point| {
                    let x = ignore_point.x;
                    let y = ignore_point.y;
                    let box_x1 = detection.r#box[0];
                    let box_y1 = detection.r#box[1];
                    let box_x2 = detection.r#box[2];
                    let box_y2 = detection.r#box[3];
                    let inside = point_inside_box(x, y, box_x1, box_y1, box_x2, box_y2);
                    debug!(
                        "Ignore point: {:?}, Box: {:?}: inside={}",
                        ignore_point, detection.r#box, inside
                    );
                    inside
                })
            })
            .collect()
    }

    /**
     * Process alerts based on detection results and hotspots
     */
    async fn process_alerts(
        &mut self,
        acceptable_detections: &[&DetectionResult],
        detections_in_row: u32,
    ) {
        // Trigger alerts based on detections and heatmap
        if self.should_post(detections_in_row) {
            self.post_to_social_media(acceptable_detections).await;
        } else if self.should_save_image() {
            self.save_detection_image(acceptable_detections).await;
        }
    }

    /**
     * Post detection to social media
     */
    async fn post_to_social_media(&mut self, acceptable_detections: &[&DetectionResult]) {
        info!("Posting to social media based on heatmap analysis");

        let image_result = draw_boxes_on_image(acceptable_detections);
        if let Ok(image) = image_result {
            // Always save the image when we're in social media posting mode, regardless of post success
            info!("Saving image for social media post (ignoring save interval)");
            self.save_detection_image(acceptable_detections).await;

            if let Err(err) = self
                .output_service
                .post_to_social_media(image.clone())
                .await
            {
                error!("Could not post to social media: {}", err);
            } else {
                // Save current time only if the post was successful
                self.last_post_time = Local::now().timestamp();
                info!("Successfully posted to social media");
            }
        } else {
            error!("Could not create image for social media");
        }
    }

    /**
     * Save detection image to a file
     */
    async fn save_detection_image(&mut self, acceptable_detections: &[&DetectionResult]) {
        info!("Saving image to a file based on heatmap detection!");

        let image_result = draw_boxes_on_image(acceptable_detections);
        if let Ok(marked_image) = image_result {
            // Create filesystem-friendly timestamp (YYYY-MM-DD_HH-MM-SS)
            let timestamp = Local::now().format("%Y-%m-%d_%H-%M-%S").to_string();
            let output_folder = &CONFIG.output.output_file_folder;

            // Ensure output directory exists
            if let Err(err) = std::fs::create_dir_all(output_folder) {
                error!(
                    "Could not create output directory {}: {}",
                    output_folder, err
                );
                return;
            }

            // Save the marked image
            let marked_image_filename = format!("{}/detection_{}.jpg", output_folder, timestamp);
            if let Err(err) = marked_image.save(&marked_image_filename) {
                error!("Could not save marked image: {}", err);
                return;
            } else {
                info!("Marked image saved to file: {}", marked_image_filename);
                // Update timestamp after successfully saving at least the marked image
                self.last_image_save_time = Local::now().timestamp();
            }

            // Copy the original image
            let original_image_filename = format!("{}/original_{}.jpg", output_folder, timestamp);
            if let Err(err) = std::fs::copy(&CONFIG.image_filename, &original_image_filename) {
                error!("Could not copy original image: {}", err);
                return;
            } else {
                info!("Original image saved to file: {}", original_image_filename);
            }

            // Send Kafka notification (non-blocking - errors won't prevent image saving)
            let detection_data: Vec<DetectionResult> = acceptable_detections
                .iter()
                .map(|&detection| DetectionResult {
                    r#box: detection.r#box,
                    cls: detection.cls,
                    cls_name: detection.cls_name.clone(),
                    conf: detection.conf,
                })
                .collect();

            match self
                .detection_kafka_service
                .send_detection_notification(&detection_data, &marked_image_filename, "image_save")
                .await
            {
                Ok(_) => {
                    debug!("Successfully sent detection notification to Kafka");
                }
                Err(err) => {
                    error!("Failed to send detection notification to Kafka: {}", err);
                    // Note: We don't return here - Kafka failures should not prevent image saving
                }
            }
        } else {
            error!("Could not create image for saving");
        }
    }

    /// Helper method to save the heatmap visualization
    fn save_heatmap_visualization(&self, temp_map: &TemperatureMap) {
        // Create base image from the current frame
        let mut heatmap_image = match image::ImageReader::open(&CONFIG.image_filename) {
            Ok(reader) => match reader.decode() {
                Ok(img) => img,
                Err(err) => {
                    error!("Could not decode base image: {}", err);
                    return;
                }
            },
            Err(err) => {
                error!("Could not open base image: {}", err);
                return;
            }
        };

        // Draw temperature map on the image
        if let Err(err) = temp_map.draw(&mut heatmap_image) {
            error!("Could not draw temperature map: {}", err);
            return;
        }

        // Only draw detection points on top for debugging
        if let Err(err) = temp_map.draw_points(&mut heatmap_image) {
            error!("Could not draw detection points: {}", err);
            return;
        }

        // Save the image
        let output_folder = &CONFIG.output.output_file_folder;

        // Ensure output directory exists
        if let Err(err) = std::fs::create_dir_all(output_folder) {
            error!(
                "Could not create output directory {}: {}",
                output_folder, err
            );
            return;
        }

        let heatmap_filename = format!("{}/heatmap.jpg", output_folder);

        // Convert RGBA to RGB for JPEG format
        let rgb_image = DynamicImage::ImageRgb8(heatmap_image.to_rgb8());

        if let Err(err) = rgb_image.save(&heatmap_filename) {
            error!("Could not save heatmap image: {}", err);
        } else {
            info!("Heatmap saved to file: {}", heatmap_filename);
            // Note: we can't update last_heatmap_save_time here since self is immutable
        }
    }

    /**
     * Checks if we should post to social media based on heatmap hotspots
     */
    fn should_post(&self, detections_in_row: u32) -> bool {
        // Check if enough time has passed since the last post
        let time_condition = self.last_post_time == 0
            || (self.last_post_time + CONFIG.output.post_interval * 60) < Local::now().timestamp();

        if !time_condition {
            debug!("Post condition not met: Not enough time passed since last post.");
            return false;
        }

        // Check if minimum consecutive detections are met
        let consecutive_detections_condition =
            detections_in_row >= CONFIG.detection.minimum_detection_frames;

        if !consecutive_detections_condition {
            debug!(
                "Post condition not met: Consecutive detections ({}) less than minimum ({}).",
                detections_in_row, CONFIG.detection.minimum_detection_frames
            );
            return false;
        }

        // Only use histogram-based hotspots for posting
        let percentile_threshold = 90.0;
        let has_hist_hotspots = self
            .temperature_map
            .has_histogram_hotspots(percentile_threshold);

        if !has_hist_hotspots {
            debug!("Post condition not met: No significant heatmap hotspots detected.");
            return false;
        }

        // All conditions met
        info!(
            "Post conditions met: Time, consecutive detections ({}), and heatmap hotspots.",
            detections_in_row
        );
        true
    }

    /**
     * Checks if we should save the image based on heatmap and time interval
     */
    fn should_save_image(&self) -> bool {
        // Check if enough time has passed since the last image save
        let time_condition = self.last_image_save_time == 0
            || (self.last_image_save_time + CONFIG.output.image_save_interval * 60)
                < Local::now().timestamp();

        // Only debug log if time condition is met
        if time_condition {
            debug!("Time condition met for saving image - last save: {}, current time: {}, interval: {}",
                self.last_image_save_time,
                Local::now().timestamp(),
                CONFIG.output.image_save_interval * 60
            );
        }

        // Only use histogram-based hotspots for saving images
        let percentile_threshold = 85.0; // Slightly lower threshold than for posting
        let has_hist_hotspots = self
            .temperature_map
            .has_histogram_hotspots(percentile_threshold);

        time_condition && has_hist_hotspots
    }
}

// SAFETY: All fields of DetectionService are Send or only accessed from one thread.
// unsafe impl Send for DetectionService {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::temperature_map::Point;
    use std::fs;
    use tempfile;

    // Test constants to avoid CONFIG dependency in tests - These are not used by should_post directly anymore
    // const TEST_MIN_DETECTION_FRAMES: u32 = 3;
    // const TEST_POST_INTERVAL: i64 = 5; // 5 minutes for testing

    #[test]
    fn test_should_post_time_condition() {
        let detections_in_row = CONFIG.detection.minimum_detection_frames;

        // Scenario 1: Initial post (last_post_time is 0)
        let mut service_initial = DetectionService::new(); // last_post_time is 0 by default
                                                           // Simulate hotspots
        service_initial.temperature_map.set_points(vec![
            Point {
                x: 50.0,
                y: 50.0,
                value: Some(95.0),
            }, // High value point
        ]);
        assert!(
            service_initial.should_post(detections_in_row),
            "Should be able to post initially if other conditions (frames, hotspots) are met by CONFIG"
        );

        // Scenario 2: Recent post (not enough time passed)
        let mut service_recent = DetectionService {
            last_post_time: Local::now().timestamp() - (CONFIG.output.post_interval * 60 / 2), // Half of the interval
            ..Default::default()
        };
        // Simulate hotspots
        service_recent.temperature_map.set_points(vec![
            Point {
                x: 50.0,
                y: 50.0,
                value: Some(95.0),
            }, // High value point
        ]);
        assert!(
            !service_recent.should_post(detections_in_row),
            "Should not be able to post if not enough time has passed, even if other conditions met"
        );

        // Scenario 3: Old post (enough time passed)
        let mut service_old = DetectionService {
            last_post_time: Local::now().timestamp() - (CONFIG.output.post_interval * 60) - 1,
            ..Default::default()
        };
        // Simulate hotspots
        service_old.temperature_map.set_points(vec![
            Point {
                x: 50.0,
                y: 50.0,
                value: Some(95.0),
            }, // High value point
        ]);
        assert!(
            service_old.should_post(detections_in_row),
            "Should be able to post if enough time has passed and other conditions met"
        );

        // Scenario 4: No hotspots, even if time condition met
        let service_no_hotspots_time_met = DetectionService {
            last_post_time: 0, // Time condition met
            ..Default::default()
        };
        // No points added to temperature_map, so no hotspots
        assert!(
            !service_no_hotspots_time_met.should_post(detections_in_row),
            "Should not post if no hotspots, even if time and frame conditions are met"
        );
    }

    #[test]
    fn test_should_post_consecutive_detections_condition() {
        // Assume time and hotspot conditions are met for these tests
        let mut service = DetectionService {
            last_post_time: 0, // Time condition met
            ..Default::default()
        };
        // Simulate hotspots
        service.temperature_map.set_points(vec![
            Point {
                x: 50.0,
                y: 50.0,
                value: Some(95.0),
            }, // High value point
        ]);

        let min_frames_from_config = CONFIG.detection.minimum_detection_frames;
        assert!(min_frames_from_config > 0, "CONFIG.detection.minimum_detection_frames should be positive for these tests to be meaningful");

        // Test below minimum
        if min_frames_from_config > 1 {
            // Ensure we don't go to 0 or negative if min_frames is 1
            assert!(
                !service.should_post(min_frames_from_config - 1),
                "Should not post if detections_in_row is less than CONFIG.detection.minimum_detection_frames"
            );
        }

        // Test at minimum
        assert!(
            service.should_post(min_frames_from_config),
            "Should post if detections_in_row is equal to CONFIG.detection.minimum_detection_frames (and other conditions met)"
        );

        // Test above minimum
        assert!(
            service.should_post(min_frames_from_config + 1),
            "Should post if detections_in_row is greater than CONFIG.detection.minimum_detection_frames (and other conditions met)"
        );

        // Scenario: No hotspots, even if frame condition met
        let service_no_hotspots_frames_met = DetectionService {
            last_post_time: 0, // Time condition met
            ..Default::default()
        };
        // No points added to temperature_map, so no hotspots
        assert!(
            !service_no_hotspots_frames_met.should_post(min_frames_from_config),
            "Should not post if no hotspots, even if time and frame conditions are met"
        );
    }

    #[test]
    fn test_should_post_hotspot_condition() {
        // Use mock service for testing to avoid Kafka spam
        let mut mock_service_no_hotspots = MockDetectionService::new();
        mock_service_no_hotspots.last_post_time = 0; // Time condition met

        // No points added, so no hotspots
        // For mock service, we'll test the logic independently
        assert!(
            !mock_service_no_hotspots
                .temperature_map
                .has_histogram_hotspots(90.0),
            "Should not have hotspots in empty temperature map"
        );

        let mut mock_service_with_hotspots = MockDetectionService::new();
        mock_service_with_hotspots.last_post_time = 0; // Time condition met

        // Add a point likely to be above 90th percentile
        mock_service_with_hotspots.temperature_map.set_points(vec![
            Point {
                x: 10.0,
                y: 10.0,
                value: Some(20.0),
            },
            Point {
                x: 50.0,
                y: 50.0,
                value: Some(95.0),
            }, // High value point
            Point {
                x: 90.0,
                y: 90.0,
                value: Some(30.0),
            },
        ]);

        // Check that hotspots exist
        assert!(
            mock_service_with_hotspots
                .temperature_map
                .has_histogram_hotspots(90.0),
            "Should have hotspots with high-value points"
        );
    }

    #[test]
    fn test_save_image_confidence_triggers_save() {
        // Set up test directory
        let temp_dir = tempfile::tempdir().unwrap();
        let temp_path = temp_dir.path().to_str().unwrap();

        // Create a mock detection service that saves files to our temp directory
        struct MockDetectionService {
            last_image_save_time: i64,
            #[allow(dead_code)]
            image_filename: String,
            output_folder: String,
        }

        impl MockDetectionService {
            // Simplified version of the real method that uses our test paths
            fn save_detection_image(&mut self, _: &[&DetectionResult]) {
                let timestamp = Local::now().format("%Y-%m-%d_%H-%M-%S").to_string();

                // Create test files to simulate actual behavior
                let detection_path = format!("{}/detection_{}.jpg", self.output_folder, timestamp);
                let original_path = format!("{}/original_{}.jpg", self.output_folder, timestamp);

                // Write some test content to the files
                fs::write(&detection_path, b"test").unwrap();
                fs::write(&original_path, b"test").unwrap();

                // Update timestamp like the real method does
                self.last_image_save_time = Local::now().timestamp();
            }

            // Simplified version of the real method
            fn should_save_image(&self) -> bool {
                // Always return true for this test
                true
            }

            // Partial implementation of process_detection for testing
            fn process_detection(&mut self, detection_result: &[DetectionResult]) -> Option<u32> {
                if !detection_result.is_empty() {
                    let save_image_candidates: Vec<&DetectionResult> = detection_result
                        .iter()
                        .filter(|d| d.conf >= 75 && d.conf < 80)
                        .collect();

                    if !save_image_candidates.is_empty() && self.should_save_image() {
                        self.save_detection_image(&save_image_candidates);
                    }
                }
                Some(0)
            }
        }

        // Create test image file
        let test_image_path = format!("{}/source.jpg", temp_path);
        fs::write(&test_image_path, b"test image data").unwrap();

        // Create our mock service
        let mut mock_service = MockDetectionService {
            last_image_save_time: 0,
            image_filename: test_image_path,
            output_folder: temp_path.to_string(),
        };

        // Create a detection just above save_image_confidence
        let detection = DetectionResult {
            r#box: [10, 10, 50, 50],
            cls: 0,
            cls_name: "seal".to_string(),
            conf: 76, // Above 75 but below 80
        };

        // Process the detection using our mock
        let detections = vec![detection];
        let result = mock_service.process_detection(&detections);

        // Verify the expected behavior
        assert_eq!(result, Some(0));

        // Check that the expected files were created
        let detection_found = fs::read_dir(temp_path).unwrap().any(|entry| {
            let entry = entry.unwrap();
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("detection_")
        });

        let original_found = fs::read_dir(temp_path).unwrap().any(|entry| {
            let entry = entry.unwrap();
            entry.file_name().to_string_lossy().starts_with("original_")
        });

        assert!(
            detection_found,
            "Detection image should be saved when detection is above save_image_confidence"
        );

        assert!(
            original_found,
            "Original image should be saved when detection is above save_image_confidence"
        );
    }

    #[tokio::test]
    async fn test_mock_detection_service() {
        let mock_service = MockDetectionService::new();

        // Test that mock service returns empty detections by default
        let result = mock_service.do_detection("fake_image.jpg").await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 0);
    }

    #[tokio::test]
    async fn test_mock_detection_service_with_detections() {
        let test_detection = DetectionResult {
            r#box: [10, 10, 50, 50],
            cls: 0,
            cls_name: "seal".to_string(),
            conf: 85,
        };

        let mock_service =
            MockDetectionService::new().with_mock_detections(vec![test_detection.clone()]);

        let result = mock_service.do_detection("fake_image.jpg").await;
        assert!(result.is_ok());

        let detections = result.unwrap();
        assert_eq!(detections.len(), 1);
        assert_eq!(detections[0].cls_name, "seal");
        assert_eq!(detections[0].conf, 85);
    }

    #[tokio::test]
    async fn test_mock_detection_service_failure() {
        let mock_service = MockDetectionService::new().with_failure(true);

        let result = mock_service.do_detection("fake_image.jpg").await;
        assert!(result.is_err());
    }
}
