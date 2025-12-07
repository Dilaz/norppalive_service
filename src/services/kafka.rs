use crate::config::CONFIG;
use crate::error::NorppaliveError;

use super::SocialMediaService;
use async_trait::async_trait;
use base64::Engine;
use rdkafka::producer::{FutureProducer, FutureRecord};
use rdkafka::ClientConfig;
use serde_json::json;
use std::time::Duration;
use tracing::{debug, info};

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
                err
            })?;

        debug!("Message sent to Kafka: {:?}", res);

        Ok(())
    }

    fn name(&self) -> &'static str {
        "Kafka"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kafka_service_name() {
        // Simple test that doesn't require actual Kafka connection
        let service = KafkaService {
            topic: "test_topic".to_string(),
            producer: {
                let mut client_config = ClientConfig::new();
                client_config.set("bootstrap.servers", "localhost:9092");
                client_config.set("socket.timeout.ms", "100");
                client_config.set("message.timeout.ms", "100");
                client_config.create().expect("Producer creation error")
            },
        };
        assert_eq!(service.name(), "Kafka");
    }
}
