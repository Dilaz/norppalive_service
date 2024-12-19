use chrono::Local;
use reqwest::{multipart, Body, Client};
use serde::{Deserialize, Deserializer, Serialize};
use tokio::fs::File;
use tokio_util::codec::{BytesCodec, FramedRead};
use tracing::{debug, error, info};
use std::sync::atomic::Ordering::Release;

use crate::{
    utils::image_utils::draw_boxes_on_image,
    CONFIG, SHUTDOWN,
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

fn float_to_u8<'de, D>(deserializer: D) -> Result<u8, D::Error>
where
    D: Deserializer<'de>,
{
    let f = f32::deserialize(deserializer)?;
    Ok((f * 1.0) as u8)
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

#[derive(Default)]
pub struct DetectionService {
    output_service: OutputService,
    last_post_time: i64,
    last_image_save_time: i64,
    last_detection_time: i64,
}

impl DetectionService {
    pub fn new() -> Self {
        Self {
            output_service: OutputService::new(),
            last_post_time: 0,
            last_image_save_time: 0,
            last_detection_time: 0,
        }
    }

    pub async fn do_detection(&self, filename: &str) -> Result<Vec<DetectionResult>, String> {
        info!("Detecting and stuff!");
        
        let file_handle = File::open(&filename).await.map_err(|err| format!("Could not open the file: {}", err))?;
        let bytes_stream = FramedRead::new(file_handle, BytesCodec::new());
        let form = multipart::Form::new()
        .part("file", multipart::Part::stream(Body::wrap_stream(bytes_stream))
        .file_name(filename.to_string())
        .mime_str("image/png")
        .expect("error"));
        println!("File loaded!");

        let client = Client::new();
        let res = client
        .post(&CONFIG.detection.api_url,)
        .multipart(form)
        .send().await;

        if let Err(err) = res {
            SHUTDOWN.store(true, Release);
            return Err(format!("Could not send the request: {}", err));
        }

        let result: Vec<DetectionResult> = res.unwrap().json().await
        .map_err(|err| format!("Could not parse detector response: {}", err))?;

        println!("{:?}", result);

        Ok(vec![])
    }

    pub async fn process_detection(&mut self, detection_result: &[DetectionResult], detections_in_row: u32) -> Option<u32> {
        if detection_result.is_empty() {
            debug!("No detections");
            return None;
        }
        info!("Found {} seals", detection_result.len());

        // Ignore some points in the image that are predefined in the config file
        let mut ignore_points_iter = CONFIG.detection.ignore_points.iter();
        let acceptable_detections: Vec<_> = detection_result
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
                    x < box_x
                        || x > box_x + box_width
                        || y < box_y
                        || y > box_y + box_height
                })
            })
            .collect();

        if acceptable_detections.is_empty() {
            debug!("No acceptable detections");
            return None;
        }

        info!(
            "Found {} acceptable detections with confidence threshold of {}",
            acceptable_detections.len(),
            CONFIG.detection.minimum_detection_percentage
        );

        self.last_detection_time = Local::now().timestamp();

        if self.should_post(detections_in_row) {
            info!("Posting to social media");
            if let Ok(image) = draw_boxes_on_image(&acceptable_detections) {
                if let Err(err) = self.output_service.post_to_social_media(image).await {
                    error!("Could not post to social media: {}", err);
                }
                // Save current time
                self.last_post_time = Local::now().timestamp();
            } else {
                error!("Could not draw boxes on image");
            }
        } else if self.should_save_image() {
            info!("Saving image to a file!");
            if let Ok(image) = draw_boxes_on_image(&acceptable_detections) {
                let timestamp = Local::now().to_string();
                image
                    .save(format!(
                        "{}/frame_{}.jpg",
                        CONFIG.output.output_file_folder,
                        timestamp
                    ))
                    .unwrap();
                info!("Image saved to file: frame_{}.jpg", timestamp);
            } else {
                error!("Could not draw boxes on image");
            }
            self.last_image_save_time = Local::now().timestamp();
        }

        let _ = draw_boxes_on_image(&acceptable_detections);

        Some(detections_in_row + 1)
    }

    /**
     * Checks if we should post to social media
     */
    fn should_post(&self, detections_in_row: u32) -> bool {
        (self.last_post_time == 0
        || (self.last_post_time + CONFIG.output.post_interval * 60) < Local::now().timestamp())
        && detections_in_row >= CONFIG.detection.minimum_detection_frames
    }

    /**
     * Checks if we should save the image
     */
    fn should_save_image(&self) -> bool {
        debug!("Last image save time: {}, current time: {}, image_save_interval: {}", self.last_image_save_time, Local::now().timestamp(), CONFIG.output.image_save_interval  * 60);
        debug!("Should save image: {}", self.last_image_save_time == 0 || (self.last_image_save_time + CONFIG.output.image_save_interval  * 60) < Local::now().timestamp());
        debug!("Time until next image save: {}", (self.last_image_save_time + CONFIG.output.image_save_interval  * 60) - Local::now().timestamp());

        self.last_image_save_time == 0
        || (self.last_image_save_time + CONFIG.output.image_save_interval  * 60) < Local::now().timestamp()
    }
}
