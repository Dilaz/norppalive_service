use crate::{
    config::Service,
    error::NorppaliveError,
    services::{
        BlueskyService, KafkaService, MastodonService, ServiceType, SocialMediaService,
        TwitterService,
    },
    CONFIG,
};
use chrono::Utc;
use futures::future;
use image::DynamicImage;
use rand::prelude::*;
use tracing::{error, info};

pub struct OutputService {
    output_services: Vec<ServiceType>,
}

impl Default for OutputService {
    fn default() -> Self {
        let mut output_services: Vec<ServiceType> = vec![];

        for service in &CONFIG.output.services {
            match service {
                Service::Twitter => {
                    info!("Adding Twitter service");
                    output_services.push(TwitterService.into());
                }
                Service::Mastodon => {
                    info!("Adding Mastodon service");
                    output_services.push(MastodonService.into());
                }
                Service::Bluesky => {
                    info!("Adding Bluesky service");
                    output_services.push(BlueskyService::default().into());
                }
                Service::Kafka => {
                    info!("Adding Kafka service");
                    output_services.push(KafkaService::default().into());
                }
            }
        }

        info!("Total output services: {}", output_services.len());

        Self { output_services }
    }
}
impl OutputService {
    pub async fn post_to_social_media(&self, image: DynamicImage) -> Result<(), NorppaliveError> {
        let services = &CONFIG.output.services;

        info!("Posting to social media services: {:?}", services);

        // Save the image
        let image_path = format!(
            "{}/image-{}.jpg",
            &CONFIG.output.output_file_folder,
            Utc::now().timestamp()
        );
        image.save(&image_path)?;

        // Get a random message
        let messages = &CONFIG.output.messages;
        let mut message = messages
            .get(rand::rng().random_range(0..messages.len()))
            .unwrap()
            .to_owned();

        // Replace # with something else if needed, for debugging
        if CONFIG.output.replace_hashtags {
            message = message.replace("#", "häsitäägi-");
        }

        // Create a vector of futures - one for each service
        let mut futures = Vec::new();

        for service in &self.output_services {
            let service_message = message.clone();
            let service_image_path = image_path.clone();

            // Create a future for each service post operation
            let future = async move {
                match service.post(&service_message, &service_image_path).await {
                    Ok(_) => {
                        info!("Posted to social media service {}", service.name());
                        true
                    }
                    Err(err) => {
                        error!("Error posting to {}: {}", service.name(), err);
                        false
                    }
                }
            };

            futures.push(future);
        }

        // Execute all service post futures concurrently
        let results = future::join_all(futures).await;

        // Check if at least one service succeeded
        if results.iter().any(|&success| success) {
            info!("Posted to at least one social media service successfully");
            Ok(())
        } else if results.is_empty() {
            info!("No social media services configured");
            Ok(())
        } else {
            error!("Failed to post to any social media service");
            Err(NorppaliveError::Other(
                "Failed to post to any social media service".to_string(),
            ))
        }
    }
}

// async fn read_from_kvstore(key: &str) -> Result<String, String> {
//     let client = reqwest::Client::new();
//     let response = client.get(format!("{}{}", &CONFIG.output.kvstore_url, key).as_str())
//         .bearer_auth(&self.kvstore_token)
//         .json(&body)
//         .send().await?;

//     if response.status() != 200 {
//         return Err("Error setting game to KVStore".into());
//     }

//     Ok(())
// }

// async fn save_to_kvstore(key: &str, value: &str) -> Result<String, String> {
//     let body = KVStoreRequest {
//         value: game.to_string(),
//     };

//     let response = self.http_client.post(format!("{}{}", KVSTORE_URL, KVSTORE_KEY).as_str())
//         .bearer_auth(&self.kvstore_token)
//         .json(&body)
//         .send().await?;

//     if response.status() != 200 {
//         return Err("Error setting game to KVStore".into());
//     }

//     Ok(())
// }
