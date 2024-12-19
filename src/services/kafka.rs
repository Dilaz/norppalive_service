use crate::CONFIG;

use super::SocialMediaService;
use base64::Engine;
use rdkafka::producer::{FutureProducer, FutureRecord};
use rdkafka::ClientConfig;
use serde_json::json;
use std::time::Duration;

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
    async fn post(&self, message: &str, image_path: &str) -> Result<(), String> {
        let image_data = std::fs::read(image_path)
            .map_err(|err| format!("Error reading image file: {:?}", err))?;
        
        let payload = json!({
            "message": message,
            "image": base64::engine::general_purpose::STANDARD.encode(image_data)
        }).to_string();

        self.producer.send(
            FutureRecord::to(&self.topic)
                .payload(&payload)
                .key("key"),
            Duration::from_secs(0),
        ).await
        .map_err(|(err, _)| format!("Error sending message to Kafka: {:?}", err))?;

        Ok(())
    }
}
