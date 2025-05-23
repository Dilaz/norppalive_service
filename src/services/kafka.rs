use crate::error::NorppaliveError;
use crate::CONFIG;

use super::SocialMediaService;
use base64::Engine;
use chrono::{DateTime, Utc};
use rdkafka::producer::{FutureProducer, FutureRecord};
use rdkafka::ClientConfig;
use serde::Serialize;
use serde_json::json;
use std::time::Duration;
use tracing::{debug, error, info};

pub struct KafkaService {
    pub topic: String,
    pub producer: FutureProducer,
}

impl Default for KafkaService {
    fn default() -> Self {
        let producer: FutureProducer = ClientConfig::new()
            .set("bootstrap.servers", &CONFIG.kafka.broker)
            .create()
            .expect("Producer creation error");

        Self {
            topic: CONFIG.kafka.topic.clone(),
            producer,
        }
    }
}

impl SocialMediaService for KafkaService {
    async fn post(&self, message: &str, image_path: &str) -> Result<(), NorppaliveError> {
        let image_data = std::fs::read(image_path)?;
        info!("Sending message to Kafka topic {}", self.topic);
        debug!("Message: {}", message);

        let base64_encoded = base64::engine::general_purpose::STANDARD.encode(image_data);
        let payload = json!({
            "message": message,
            "image": base64_encoded
        })
        .to_string();

        let res = self
            .producer
            .send(
                FutureRecord::to(&self.topic).payload(&payload).key("key"),
                Duration::from_secs(10),
            )
            .await
            .map_err(|(err, _)| {
                info!("Kafka error details: {}", err);
                err
            })?;

        debug!("Message sent to Kafka: {:?}", res);

        Ok(())
    }

    fn name(&self) -> &'static str {
        "Kafka"
    }
}

#[derive(Serialize, Debug)]
pub struct DetectionNotification {
    pub timestamp: DateTime<Utc>,
    pub detection_type: String,
    pub confidence_level: Option<u8>,
    pub detection_count: usize,
    pub image_filename: String,
    pub detections: Vec<DetectionInfo>,
}

#[derive(Serialize, Debug)]
pub struct DetectionInfo {
    pub r#box: [u32; 4],
    pub confidence: u8,
    pub class: String,
}

pub struct DetectionKafkaService {
    pub topic: String,
    pub producer: FutureProducer,
}

impl Default for DetectionKafkaService {
    fn default() -> Self {
        let producer: FutureProducer = ClientConfig::new()
            .set("bootstrap.servers", &CONFIG.kafka.broker)
            .create()
            .expect("Detection Kafka producer creation error");

        Self {
            topic: CONFIG.kafka.detection_topic.clone(),
            producer,
        }
    }
}

impl DetectionKafkaService {
    pub async fn send_detection_notification(
        &self,
        detections: &[crate::utils::detection_utils::DetectionResult],
        image_filename: &str,
        detection_type: &str,
    ) -> Result<(), NorppaliveError> {
        let detection_infos: Vec<DetectionInfo> = detections
            .iter()
            .map(|d| DetectionInfo {
                r#box: d.r#box,
                confidence: d.conf,
                class: d.cls_name.clone(),
            })
            .collect();

        let max_confidence = detections.iter().map(|d| d.conf).max();

        let notification = DetectionNotification {
            timestamp: Utc::now(),
            detection_type: detection_type.to_string(),
            confidence_level: max_confidence,
            detection_count: detections.len(),
            image_filename: image_filename.to_string(),
            detections: detection_infos,
        };

        let payload = serde_json::to_string(&notification).map_err(|e| {
            NorppaliveError::Other(format!("Failed to serialize detection notification: {}", e))
        })?;

        info!(
            "Sending detection notification to Kafka topic {}",
            self.topic
        );
        debug!("Detection notification payload: {}", payload);

        let res = self
            .producer
            .send(
                FutureRecord::to(&self.topic)
                    .payload(&payload)
                    .key(&format!("detection_{}", notification.timestamp.timestamp())),
                Duration::from_secs(5), // Shorter timeout for detection notifications
            )
            .await
            .map_err(|(err, _)| {
                error!("Detection Kafka error details: {}", err);
                NorppaliveError::Other(format!(
                    "Failed to send detection notification to Kafka: {}",
                    err
                ))
            })?;

        debug!("Detection notification sent to Kafka: {:?}", res);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::detection_utils::DetectionResult;

    #[test]
    fn test_detection_notification_serialization() {
        let detection_result = DetectionResult {
            r#box: [10, 20, 30, 40],
            cls: 0,
            cls_name: "seal".to_string(),
            conf: 85,
        };

        let detection_infos: Vec<DetectionInfo> = vec![DetectionInfo {
            r#box: detection_result.r#box,
            confidence: detection_result.conf,
            class: detection_result.cls_name.clone(),
        }];

        let notification = DetectionNotification {
            timestamp: Utc::now(),
            detection_type: "image_save".to_string(),
            confidence_level: Some(85),
            detection_count: 1,
            image_filename: "test_image.jpg".to_string(),
            detections: detection_infos,
        };

        // Test that the notification can be serialized to JSON
        let json_result = serde_json::to_string(&notification);
        assert!(json_result.is_ok());

        let json_string = json_result.unwrap();
        assert!(json_string.contains("image_save"));
        assert!(json_string.contains("test_image.jpg"));
        assert!(json_string.contains("seal"));
        assert!(json_string.contains("85"));
    }

    #[test]
    fn test_detection_info_creation() {
        let detection_result = DetectionResult {
            r#box: [100, 200, 300, 400],
            cls: 1,
            cls_name: "test_class".to_string(),
            conf: 75,
        };

        let detection_info = DetectionInfo {
            r#box: detection_result.r#box,
            confidence: detection_result.conf,
            class: detection_result.cls_name.clone(),
        };

        assert_eq!(detection_info.r#box, [100, 200, 300, 400]);
        assert_eq!(detection_info.confidence, 75);
        assert_eq!(detection_info.class, "test_class");
    }
}
