use chrono::Utc;
use image::DynamicImage;
use rand::prelude::*;
use tracing::{error, info};
use crate::{config::Service, error::NorppaliveError, services::{BlueskyService, KafkaService, MastodonService, ServiceType, SocialMediaService, TwitterService}, CONFIG};

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

        Self {
            output_services,
        }
    }
}
impl OutputService {
    pub async fn post_to_social_media(&self, image: DynamicImage) -> Result<(), NorppaliveError> {
        let services = &CONFIG.output.services;

        info!("Posting to social media services: {:?}", services);

        // Save the image
        let image_path = format!("{}/image-{}.jpg", &CONFIG.output.output_file_folder, Utc::now().timestamp());
        image.save(&image_path)?;

        // Get a random message
        let messages = &CONFIG.output.messages;
        let mut message = messages.get(rand::thread_rng().gen_range(0..messages.len())).unwrap().to_owned();

        // Replace # with something else if needed, for debugging
        if CONFIG.output.replace_hashtags {
            message = message.replace("#", "häsitäägi-");
        }

        let futures = self.output_services.iter()
            .map(|service| (service, service.post(&message, &image_path)));

        for (service, future) in futures {
            async {
                match future.await {
                    Ok(_) => info!("Posted to social media service {:?}", service.name()),
                    Err(err) => error!("Error posting to {}: {}", service.name(), err),
                }
            }.await;
        }

        Ok(())
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
