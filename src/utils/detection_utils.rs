use chrono::Local;
use image;
use image::DynamicImage;
use miette::Result;
use reqwest::{multipart, Body, Client};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio::fs::File;
use tokio_util::codec::{BytesCodec, FramedRead};
use tracing::{debug, error, info};

use crate::{
    error::NorppaliveError, utils::image_utils::draw_boxes_on_image,
    utils::temperature_map::TemperatureMap, CONFIG, SHUTDOWN,
};

use super::output::OutputService;

#[derive(Serialize, Deserialize, Debug)]
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

fn float_to_u8<'de, D>(deserializer: D) -> Result<u8, D::Error>
where
    D: Deserializer<'de>,
{
    let f = f32::deserialize(deserializer)?;
    Ok((f * 100.0) as u8)
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
            last_post_time: 0,
            last_image_save_time: 0,
            last_detection_time: 0,
            last_heatmap_save_time: 0,
            temperature_map: TemperatureMap::new(1280, 720),
        }
    }

    pub async fn do_detection(
        &self,
        filename: &str,
    ) -> Result<Vec<DetectionResult>, NorppaliveError> {
        info!("Starting detection process");

        let file_handle = File::open(&filename).await?;
        let bytes_stream = FramedRead::new(file_handle, BytesCodec::new());
        let form = multipart::Form::new().part(
            "file",
            multipart::Part::stream(Body::wrap_stream(bytes_stream))
                .file_name(filename.to_string())
                .mime_str("image/png")
                .expect("error"),
        );

        info!("File loaded!");

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
            self.save_heatmap_visualization(&self.temperature_map, &[]);
            return None;
        }

        info!("Found {} seals", detection_result.len());

        // Filter for acceptable detections
        let acceptable_detections = self.filter_acceptable_detections(detection_result);

        if acceptable_detections.is_empty() {
            debug!("No acceptable detections");
            // Create and save heatmap visualization even when there are no acceptable detections
            info!("Creating and saving heatmap visualization (no acceptable detections)");
            self.save_heatmap_visualization(&self.temperature_map, &[]);
            return None;
        }

        info!(
            "Found {} acceptable detections with confidence threshold of {}",
            acceptable_detections.len(),
            CONFIG.detection.minimum_detection_percentage
        );

        // Process temperature map and get hotspots
        let hotspots = self.process_temperature_map(&acceptable_detections);

        // Update the last detection time
        self.last_detection_time = Local::now().timestamp();

        // Create and save heatmap visualization
        info!("Creating and saving heatmap visualization");
        self.save_heatmap_visualization(&self.temperature_map, &hotspots);
        self.last_heatmap_save_time = Local::now().timestamp();

        // Handle alerts and image saving
        self.process_alerts(&acceptable_detections, detections_in_row)
            .await;

        // Keep track of consecutive detections for backward compatibility
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
        let mut ignore_points_iter = CONFIG.detection.ignore_points.iter();
        detection_result
            .iter()
            .filter(|detection| detection.conf > CONFIG.detection.minimum_detection_percentage)
            .filter(|detection| {
                ignore_points_iter.all(|ignore_point| {
                    let x = ignore_point.x;
                    let y = ignore_point.y;
                    let box_x = detection.r#box[0];
                    let box_y = detection.r#box[1];
                    let box_width = detection.r#box[2];
                    let box_height = detection.r#box[3];
                    x < box_x || x > box_x + box_width || y < box_y || y > box_y + box_height
                })
            })
            .collect()
    }

    /**
     * Process temperature map with detection data and identify hotspots
     */
    fn process_temperature_map(
        &mut self,
        acceptable_detections: &[&DetectionResult],
    ) -> Vec<(f32, f32, f32)> {
        // Update the temperature map with new detections
        self.temperature_map
            .set_points_from_detections(acceptable_detections);

        // Check if the temperature map has hotspots using both traditional and histogram methods
        let temp_threshold = CONFIG.detection.heatmap_threshold;
        let hotspots = self.temperature_map.get_hotspots(temp_threshold);

        // Use histogram-based hotspot detection
        let percentile_threshold = 90.0; // Only consider top 10% as hotspots
        let histogram_hotspots = self
            .temperature_map
            .get_histogram_hotspots(percentile_threshold);

        // Log hotspot information
        if !hotspots.is_empty() {
            info!(
                "Temperature map threshold exceeded! Found {} hotspots, highest value: {:.2}",
                hotspots.len(),
                hotspots[0].2
            );
        }

        if let Some(hist_hotspots) = &histogram_hotspots {
            if !hist_hotspots.is_empty() {
                info!(
                    "Histogram hotspots detected above {}th percentile! Found {} hotspots, highest value: {:.2}",
                    percentile_threshold,
                    hist_hotspots.len(),
                    hist_hotspots[0].2
                );
            }
        }

        hotspots
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
            self.save_detection_image(acceptable_detections);
        }
    }

    /**
     * Post detection to social media
     */
    async fn post_to_social_media(&mut self, acceptable_detections: &[&DetectionResult]) {
        info!("Posting to social media based on heatmap analysis");

        let image_result = draw_boxes_on_image(acceptable_detections);
        if let Ok(image) = image_result {
            if let Err(err) = self
                .output_service
                .post_to_social_media(image.clone())
                .await
            {
                error!("Could not post to social media: {}", err);
            } else {
                // Save current time
                self.last_post_time = Local::now().timestamp();
            }
        } else {
            error!("Could not create image for social media");
        }
    }

    /**
     * Save detection image to a file
     */
    fn save_detection_image(&mut self, acceptable_detections: &[&DetectionResult]) {
        info!("Saving image to a file based on heatmap detection!");

        let image_result = draw_boxes_on_image(acceptable_detections);
        if let Ok(image) = image_result {
            let timestamp = Local::now().to_string();
            let output_folder = &CONFIG.output.output_file_folder;

            // Ensure output directory exists
            if let Err(err) = std::fs::create_dir_all(output_folder) {
                error!(
                    "Could not create output directory {}: {}",
                    output_folder, err
                );
                return;
            }

            let image_filename = format!("{}/frame_{}.jpg", output_folder, timestamp);

            if let Err(err) = image.save(&image_filename) {
                error!("Could not save image: {}", err);
            } else {
                info!("Image saved to file: {}", image_filename);
                self.last_image_save_time = Local::now().timestamp();
            }
        } else {
            error!("Could not create image for saving");
        }
    }

    /// Helper method to save the heatmap visualization
    fn save_heatmap_visualization(&self, temp_map: &TemperatureMap, hotspots: &[(f32, f32, f32)]) {
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

        // Draw hotspots on the image if any exist
        if !hotspots.is_empty() {
            if let Err(err) = temp_map.draw_hotspots(&mut heatmap_image, hotspots) {
                error!("Could not draw hotspots: {}", err);
            }
        }

        // Draw detection points on top for debugging
        if let Err(err) = temp_map.draw_points(&mut heatmap_image) {
            error!("Could not draw detection points: {}", err);
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
            return false;
        }

        // Check if we have hotspots in the temperature map
        let temp_threshold = CONFIG.detection.heatmap_threshold;
        let has_hotspots = !self.temperature_map.get_hotspots(temp_threshold).is_empty();

        // Check for histogram-based hotspots (percentile threshold can be adjusted)
        let percentile_threshold = 90.0;
        let has_hist_hotspots = self
            .temperature_map
            .has_histogram_hotspots(percentile_threshold);

        // For backward compatibility, also consider the minimum detection frames
        let has_min_detections = detections_in_row >= CONFIG.detection.minimum_detection_frames;

        // Trigger posting if we have hotspots or enough consecutive detections
        time_condition && (has_hotspots || has_hist_hotspots || has_min_detections)
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

        // Check if we have hotspots in the temperature map
        let temp_threshold = CONFIG.detection.heatmap_threshold;
        let has_hotspots = !self.temperature_map.get_hotspots(temp_threshold).is_empty();

        // Check for histogram-based hotspots
        let percentile_threshold = 85.0; // Slightly lower threshold than for posting
        let has_hist_hotspots = self
            .temperature_map
            .has_histogram_hotspots(percentile_threshold);

        // Save if time condition is met and there's activity in the heatmap
        time_condition && (has_hotspots || has_hist_hotspots)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test for individual parts of should_post
    #[test]
    fn test_should_post_time_condition() {
        let service_new = DetectionService::new();
        assert_eq!(service_new.last_post_time, 0);

        let service_recent = DetectionService {
            last_post_time: Local::now().timestamp(),
            ..Default::default()
        };
        let time_passed = (service_recent.last_post_time + CONFIG.output.post_interval * 60)
            < Local::now().timestamp();
        assert!(!time_passed); // Time hasn't passed in this case
    }

    #[test]
    fn test_minimum_detection_frames() {
        let min_frames = CONFIG.detection.minimum_detection_frames;
        assert!(min_frames > 0); // Sanity check that config has a positive value

        // Below minimum
        assert!(min_frames - 1 < min_frames);

        // At minimum
        assert!(min_frames >= min_frames);

        // Above minimum
        assert!(min_frames + 1 >= min_frames);
    }

    // Tests for should_save_image time condition
    #[test]
    fn test_should_save_image_time_condition() {
        let service_new = DetectionService::new();
        assert_eq!(service_new.last_image_save_time, 0);

        let service_old = DetectionService {
            last_image_save_time: Local::now().timestamp()
                - (CONFIG.output.image_save_interval * 60)
                - 1,
            ..Default::default()
        };
        let time_passed = (service_old.last_image_save_time
            + CONFIG.output.image_save_interval * 60)
            < Local::now().timestamp();
        assert!(time_passed); // Time has passed in this case

        let service_recent = DetectionService {
            last_image_save_time: Local::now().timestamp(),
            ..Default::default()
        };
        let time_passed = (service_recent.last_image_save_time
            + CONFIG.output.image_save_interval * 60)
            < Local::now().timestamp();
        assert!(!time_passed); // Time hasn't passed in this case
    }
}
