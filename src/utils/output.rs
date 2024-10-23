use chrono::Utc;
use image::DynamicImage;
use rand::prelude::*;
use tracing::info;
use crate::{config::Service, services::{BlueskyService, MastodonService, ServiceType, SocialMediaService, TwitterService}, CONFIG};

pub struct OutputService {
    output_services: Vec<ServiceType>,
}

impl OutputService {
    pub fn new() -> Self {
        let mut output_services: Vec<ServiceType> = vec![];

        for service in &CONFIG.output.services {
            match service {
                Service::Twitter => {
                    info!("Adding Twitter service");
                    output_services.push(TwitterService::new().into());
                }
                Service::Mastodon => {
                    info!("Adding Mastodon service");
                    output_services.push(MastodonService::new().into());
                }
                Service::Bluesky => {
                    info!("Adding Bluesky service");
                    output_services.push(BlueskyService::new( ).into());
                }
            }
        }
        
        info!("Total output services: {}", output_services.len());

        Self {
            output_services,
        }
    }

    pub async fn post_to_social_media(&self, image: DynamicImage) -> Result<(), String> {
        let services = &CONFIG.output.services;

        info!("Posting to social media services: {:?}", services);

        // Save the image
        let image_path = format!("{}/image-{}.jpg", &CONFIG.output.output_file_folder, Utc::now().timestamp());
        image.save(&image_path).map_err(|err| format!("Error saving image: {}", err))?;

        // Get a random message
        let messages = &CONFIG.output.messages;
        let message = messages.get(rand::thread_rng().gen_range(0..messages.len())).unwrap();

        for service in self.output_services.iter() {
           service.post(message, &image_path).await?;
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
