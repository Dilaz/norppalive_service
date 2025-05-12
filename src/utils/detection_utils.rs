use chrono::Local;
use image;
use image::DynamicImage;
use miette::Result;
use reqwest::{multipart, Client};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use tracing::{debug, error, info};

use crate::{
    error::NorppaliveError, utils::image_utils::draw_boxes_on_image,
    utils::temperature_map::TemperatureMap, CONFIG, SHUTDOWN,
};

use super::output::OutputService;

const CONFIDENCE_MULTIPLIER: f32 = 100.0;

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
            .filter(|d| d.conf >= CONFIG.detection.save_image_confidence && d.conf < CONFIG.detection.minimum_detection_percentage)
            .collect();
        if !save_image_candidates.is_empty() {
            // Check image save interval
            let time_condition = self.last_image_save_time == 0
                || (self.last_image_save_time + CONFIG.output.image_save_interval * 60)
                    < Local::now().timestamp();
            if time_condition {
                info!("Saving image due to save_image_confidence threshold");
                self.save_detection_image(&save_image_candidates);
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
            } else {
                info!("Marked image saved to file: {}", marked_image_filename);
            }

            // Copy the original image
            let original_image_filename = format!("{}/original_{}.jpg", output_folder, timestamp);
            if let Err(err) = std::fs::copy(&CONFIG.image_filename, &original_image_filename) {
                error!("Could not copy original image: {}", err);
            } else {
                info!("Original image saved to file: {}", original_image_filename);
                self.last_image_save_time = Local::now().timestamp();
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

#[cfg(test)]
mod tests {
    use crate::utils::temperature_map::Point;
    use super::*;
    use std::fs;

    #[test]
    fn test_should_post_time_condition() {
        // Assume other conditions are met for this test
        let detections_in_row = CONFIG.detection.minimum_detection_frames; // Meet minimum frames

        let service_new = DetectionService::new();
        assert_eq!(service_new.last_post_time, 0);

        // For this test, we assume hotspots exist. A more robust test would mock TemperatureMap.
        let mut service_new_with_hotspots = service_new;
        // Simulate adding a point to potentially create a hotspot state (crude simulation)
        service_new_with_hotspots.temperature_map.set_points(vec![
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
        assert!(
            service_new_with_hotspots.should_post(detections_in_row),
            "Should post when time is 0"
        );

        let service_recent = DetectionService {
            last_post_time: Local::now().timestamp(),
            ..Default::default()
        };

        // Simulate hotspots again for the recent post scenario
        let mut service_recent_with_hotspots = service_recent;
        service_recent_with_hotspots
            .temperature_map
            .set_points(vec![
                Point {
                    x: 10.0,
                    y: 10.0,
                    value: Some(20.0),
                },
                Point {
                    x: 50.0,
                    y: 50.0,
                    value: Some(95.0),
                },
                Point {
                    x: 90.0,
                    y: 90.0,
                    value: Some(30.0),
                },
            ]);
        assert!(
            !service_recent_with_hotspots.should_post(detections_in_row),
            "Should not post when time interval not met"
        );

        // Simulate enough time passing
        let mut service_old = DetectionService {
            last_post_time: Local::now().timestamp() - (CONFIG.output.post_interval * 60) - 1,
            ..Default::default()
        };
        service_old.temperature_map.set_points(vec![
            Point {
                x: 10.0,
                y: 10.0,
                value: Some(20.0),
            },
            Point {
                x: 50.0,
                y: 50.0,
                value: Some(95.0),
            },
            Point {
                x: 90.0,
                y: 90.0,
                value: Some(30.0),
            },
        ]);
        assert!(
            service_old.should_post(detections_in_row),
            "Should post when time interval is met"
        );
    }

    #[test]
    fn test_should_post_consecutive_detections_condition() {
        // Assume time and hotspot conditions are met
        let mut service = DetectionService {
            last_post_time: 0, // Time condition met
            ..Default::default()
        };
        // Simulate hotspots
        service.temperature_map.set_points(vec![
            Point {
                x: 10.0,
                y: 10.0,
                value: Some(20.0),
            },
            Point {
                x: 50.0,
                y: 50.0,
                value: Some(95.0),
            },
            Point {
                x: 90.0,
                y: 90.0,
                value: Some(30.0),
            },
        ]);

        let min_frames = CONFIG.detection.minimum_detection_frames;
        assert!(min_frames > 0, "Minimum frames should be positive");

        // Test below minimum
        if min_frames > 1 {
            assert!(
                !service.should_post(min_frames - 1),
                "Should not post when consecutive frames are less than minimum"
            );
        }

        // Test at minimum
        assert!(
            service.should_post(min_frames),
            "Should post when consecutive frames meet minimum"
        );

        // Test above minimum
        assert!(
            service.should_post(min_frames + 1),
            "Should post when consecutive frames exceed minimum"
        );
    }

    #[test]
    fn test_should_post_hotspot_condition() {
        // Assume time and consecutive detections conditions are met
        let detections_in_row = CONFIG.detection.minimum_detection_frames;
        let service_no_hotspots = DetectionService {
            last_post_time: 0, // Time condition met
            ..Default::default()
        };
        // No points added, so no hotspots
        assert!(
            !service_no_hotspots.should_post(detections_in_row),
            "Should not post without heatmap hotspots"
        );

        let mut service_with_hotspots = DetectionService {
            last_post_time: 0, // Time condition met
            ..Default::default()
        };
        // Add a point likely to be above 90th percentile
        service_with_hotspots.temperature_map.set_points(vec![
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
        assert!(
            service_with_hotspots.should_post(detections_in_row),
            "Should post with heatmap hotspots"
        );
    }

    #[test]
    fn test_minimum_detection_frames() {
        let min_frames = CONFIG.detection.minimum_detection_frames;
        assert!(min_frames > 0); // Sanity check that config has a positive value
    }

    #[test]
    fn test_save_image_confidence_triggers_save() {
        let mut service = DetectionService::default();
        // Set config thresholds
        let save_conf = CONFIG.detection.save_image_confidence;
        // Create a detection result just above save_image_confidence but below minimum_detection_percentage
        let detection = DetectionResult {
            r#box: [10, 10, 50, 50],
            cls: 0,
            cls_name: "seal".to_string(),
            conf: save_conf + 1,
        };
        let detections = vec![detection];
        // Remove any previously saved test image
        let output_folder = &CONFIG.output.output_file_folder;
        let pattern = format!("{}/frame_", output_folder);
        let _ = fs::create_dir_all(output_folder);
        for entry in fs::read_dir(output_folder).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.to_string_lossy().contains(&pattern) {
                let _ = fs::remove_file(path);
            }
        }
        // Call process_detection
        let result = futures::executor::block_on(service.process_detection(&detections, 0));
        // It should not increment detections_in_row
        assert_eq!(result, Some(0));
        // Check that an image was saved
        let found = fs::read_dir(output_folder).unwrap().any(|entry| {
            let entry = entry.unwrap();
            entry.file_name().to_string_lossy().starts_with("frame_")
        });
        assert!(found, "Image should be saved when detection is above save_image_confidence");
    }
}
