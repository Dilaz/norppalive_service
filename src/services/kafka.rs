use crate::config::CONFIG;
use crate::error::NorppaliveError;

use super::SocialMediaService;
use async_trait::async_trait;
use base64::Engine;
use chrono::Utc;
use rdkafka::producer::{FutureProducer, FutureRecord};
use rdkafka::ClientConfig;
use serde_json::json;
use std::time::Duration;
use tracing::{debug, error, info};

#[derive(Clone)]
pub struct KafkaService {
    pub topic: String,
    pub producer: FutureProducer,
}

impl Default for KafkaService {
    fn default() -> Self {
        let mut client_config = ClientConfig::new();
        client_config.set("bootstrap.servers", &CONFIG.kafka.broker);

        #[cfg(test)]
        {
            client_config.set("socket.timeout.ms", "2000");
            client_config.set("message.timeout.ms", "2000"); // For producer
        }

        let producer: FutureProducer = client_config.create().expect("Producer creation error");

        Self {
            topic: CONFIG.kafka.topic.clone(),
            producer,
        }
    }
}

#[async_trait]
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
                NorppaliveError::KafkaError(err)
            })?;

        debug!("Message sent to Kafka: {:?}", res);

        Ok(())
    }

    fn name(&self) -> &'static str {
        "Kafka"
    }
}

// DetectionNotification and DetectionInfo structs are removed from here.

// Trait for Kafka service abstraction
pub trait DetectionKafkaServiceTrait {
    async fn send_detection_notification(
        &self,
        _detections: &[crate::utils::detection_utils::DetectionResult],
        image_filename: &str,
        detection_type: &str,
    ) -> Result<(), NorppaliveError>;
}

// Mock implementation for testing
#[cfg(test)]
pub struct MockDetectionKafkaService {
    pub should_fail: bool,
    pub messages_sent: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}

#[cfg(test)]
impl MockDetectionKafkaService {
    pub fn new() -> Self {
        Self {
            should_fail: false,
            messages_sent: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }

    pub fn with_failure(mut self, should_fail: bool) -> Self {
        self.should_fail = should_fail;
        self
    }

    pub fn get_messages_sent(&self) -> Vec<String> {
        self.messages_sent.lock().unwrap().clone()
    }

    pub fn get_message_count(&self) -> usize {
        self.messages_sent.lock().unwrap().len()
    }
}

#[cfg(test)]
impl DetectionKafkaServiceTrait for MockDetectionKafkaService {
    async fn send_detection_notification(
        &self,
        _detections: &[crate::utils::detection_utils::DetectionResult],
        image_filename: &str,
        detection_type: &str,
    ) -> Result<(), NorppaliveError> {
        if self.should_fail {
            return Err(NorppaliveError::Other("Mock Kafka failure".to_string()));
        }

        let mock_message = format!(
            "Mock Kafka static message: type '{}', image: '{}'",
            detection_type, image_filename
        );

        {
            let mut messages = self.messages_sent.lock().unwrap();
            messages.push(mock_message.clone());
        }

        info!("Mock: Sent Kafka message - {}", mock_message);
        Ok(())
    }
}

#[derive(Clone)]
pub struct DetectionKafkaService {
    pub topic: String,
    pub producer: FutureProducer,
}

impl Default for DetectionKafkaService {
    fn default() -> Self {
        let mut client_config = ClientConfig::new();
        client_config.set("bootstrap.servers", &CONFIG.kafka.broker);

        #[cfg(test)]
        {
            client_config.set("socket.timeout.ms", "2000");
            client_config.set("message.timeout.ms", "2000"); // For producer
        }

        let producer: FutureProducer = client_config
            .create()
            .expect("Detection Kafka producer creation error");

        Self {
            topic: CONFIG.kafka.detection_topic.clone(),
            producer,
        }
    }
}

impl DetectionKafkaServiceTrait for DetectionKafkaService {
    async fn send_detection_notification(
        &self,
        _detections: &[crate::utils::detection_utils::DetectionResult],
        image_filename: &str,
        _detection_type: &str,
    ) -> Result<(), NorppaliveError> {
        let image_data = std::fs::read(image_filename).map_err(|e| {
            error!("Failed to read image file {}: {}", image_filename, e);
            NorppaliveError::Other(format!(
                "Failed to read image file {}: {}",
                image_filename, e
            ))
        })?;

        let image_size = image_data.len();
        let base64_encoded = base64::engine::general_purpose::STANDARD.encode(image_data);

        let configured_message = &CONFIG.kafka.detection_message;

        let payload = json!({
            "message": configured_message,
            "image": base64_encoded
        })
        .to_string();

        let timestamp = Utc::now();
        let key = format!("detection_{}", timestamp.timestamp());

        info!(
            "Sending detection notification to Kafka topic {} with static message",
            self.topic
        );
        debug!(
            "Detection notification: message='{}', image_size={} bytes, payload_size={} bytes",
            configured_message,
            image_size,
            payload.len()
        );

        let res = self
            .producer
            .send(
                FutureRecord::to(&self.topic).payload(&payload).key(&key),
                Duration::from_secs(5),
            )
            .await
            .map_err(|(err, _)| {
                error!("Detection Kafka error details: {}", err);
                NorppaliveError::KafkaError(err)
            })?;

        debug!("Detection notification sent to Kafka: {:?}", res);
        Ok(())
    }
}

impl DetectionKafkaService {
    // Keep the original method for backward compatibility
    pub async fn send_detection_notification(
        &self,
        detections: &[crate::utils::detection_utils::DetectionResult],
        image_filename: &str,
        detection_type: &str,
    ) -> Result<(), NorppaliveError> {
        DetectionKafkaServiceTrait::send_detection_notification(
            self,
            detections,
            image_filename,
            detection_type,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::detection_utils::DetectionResult;

    #[test]
    fn test_mock_kafka_service() {
        let mock_service = MockDetectionKafkaService::new();
        assert_eq!(mock_service.get_message_count(), 0);
        assert!(!mock_service.should_fail);
    }

    #[tokio::test]
    async fn test_mock_kafka_send_detection() {
        let mock_service = MockDetectionKafkaService::new();

        let detection = DetectionResult {
            r#box: [10, 20, 30, 40],
            cls: 0,
            cls_name: "seal".to_string(),
            conf: 85,
        };

        let result = mock_service
            .send_detection_notification(&[detection], "test_image.jpg", "test_event_type")
            .await;

        assert!(result.is_ok());
        assert_eq!(mock_service.get_message_count(), 1);

        let messages = mock_service.get_messages_sent();
        assert!(messages[0].contains(
            "Mock Kafka static message: type 'test_event_type', image: 'test_image.jpg'"
        ));
    }

    #[tokio::test]
    async fn test_mock_kafka_failure() {
        let mock_service = MockDetectionKafkaService::new().with_failure(true);

        let detection = DetectionResult {
            r#box: [10, 20, 30, 40],
            cls: 0,
            cls_name: "seal".to_string(),
            conf: 85,
        };

        let result = mock_service
            .send_detection_notification(&[detection], "test_image.jpg", "test_event")
            .await;

        assert!(result.is_err());
        assert_eq!(mock_service.get_message_count(), 0);
    }
}
