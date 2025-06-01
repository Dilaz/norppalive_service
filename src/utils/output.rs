use crate::{
    config::{Service, CONFIG},
    error::NorppaliveError,
    services::{BlueskyService, KafkaService, MastodonService, ServiceType, TwitterService},
};
use image::DynamicImage;
use tracing::info;

#[cfg(not(test))]
use crate::services::SocialMediaService;
#[cfg(not(test))]
use chrono::Utc;
#[cfg(not(test))]
use rand::prelude::*;

// Trait for output service abstraction
pub trait OutputServiceTrait {
    fn post_to_social_media(
        &self,
        image: DynamicImage,
    ) -> impl std::future::Future<Output = Result<(), NorppaliveError>> + Send;
}

// Mock implementation for testing
#[cfg(test)]
pub struct MockOutputService {
    pub should_fail: bool,
    pub call_count: std::sync::Arc<std::sync::Mutex<u32>>,
}

#[cfg(test)]
impl Default for MockOutputService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
impl MockOutputService {
    pub fn new() -> Self {
        Self {
            should_fail: false,
            call_count: std::sync::Arc::new(std::sync::Mutex::new(0)),
        }
    }

    pub fn with_failure(mut self, should_fail: bool) -> Self {
        self.should_fail = should_fail;
        self
    }

    pub fn get_call_count(&self) -> u32 {
        *self.call_count.lock().unwrap()
    }
}

#[cfg(test)]
impl OutputServiceTrait for MockOutputService {
    #[allow(clippy::manual_async_fn)]
    fn post_to_social_media(
        &self,
        _image: DynamicImage,
    ) -> impl std::future::Future<Output = Result<(), NorppaliveError>> + Send {
        let should_fail = self.should_fail;
        let call_count = self.call_count.clone();
        async move {
            {
                let mut count = call_count.lock().unwrap();
                *count += 1;
            }

            if should_fail {
                return Err(NorppaliveError::Other("Mock posting failure".to_string()));
            }

            info!("Mock: Posted to social media successfully");
            Ok(())
        }
    }
}

#[derive(Clone)]
pub struct OutputService {
    output_services: Vec<ServiceType>,
}

impl OutputServiceTrait for OutputService {
    #[allow(clippy::manual_async_fn)]
    fn post_to_social_media(
        &self,
        image: DynamicImage,
    ) -> impl std::future::Future<Output = Result<(), NorppaliveError>> + Send {
        let services = self.output_services.clone();
        async move {
            // Only post if not running tests
            #[cfg(test)]
            {
                let _services = services; // Avoid unused variable warning
                let _image = image; // Avoid unused variable warning
                info!("Skipping actual social media posting during tests");
                Ok(())
            }

            #[cfg(not(test))]
            {
                info!("Posting to {} social media services", services.len());

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

                // Post to all configured services
                let mut last_error = None;
                let mut success_count = 0;

                for service in &services {
                    match service.post(&message, &image_path).await {
                        Ok(()) => {
                            success_count += 1;
                            info!("Successfully posted to {}", service.name());
                        }
                        Err(e) => {
                            info!("Failed to post to {}: {:?}", service.name(), e);
                            last_error = Some(e);
                        }
                    }
                }

                info!(
                    "Posted successfully to {}/{} services",
                    success_count,
                    services.len()
                );

                // If all services failed, return the last error
                if success_count == 0 {
                    if let Some(error) = last_error {
                        return Err(error);
                    } else {
                        return Err(NorppaliveError::Other(
                            "No services were able to post".to_string(),
                        ));
                    }
                }

                Ok(())
            }
        }
    }
}

impl Default for OutputService {
    fn default() -> Self {
        let mut output_services: Vec<ServiceType> = vec![];

        for service_config in &CONFIG.output.services {
            // Assuming CONFIG is accessible here
            match service_config {
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
        info!(
            "Total output services configured: {}",
            output_services.len()
        );
        Self { output_services }
    }
}

impl OutputService {
    // Keep the original method for backward compatibility
    pub async fn post_to_social_media(&self, image: DynamicImage) -> Result<(), NorppaliveError> {
        OutputServiceTrait::post_to_social_media(self, image).await
    }

    // Getter method to access services
    pub fn get_services(&self) -> &Vec<ServiceType> {
        &self.output_services
    }

    // Method to create a new service with the same services (for cloning)
    pub fn with_services(services: Vec<ServiceType>) -> Self {
        Self {
            output_services: services,
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

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, RgbImage};

    fn create_test_image() -> DynamicImage {
        let img = RgbImage::new(100, 100);
        DynamicImage::ImageRgb8(img)
    }

    #[tokio::test]
    async fn test_mock_output_service() {
        let mock_service = MockOutputService::new();
        assert_eq!(mock_service.get_call_count(), 0);

        let test_image = create_test_image();
        let result = mock_service.post_to_social_media(test_image).await;

        assert!(result.is_ok());
        assert_eq!(mock_service.get_call_count(), 1);
    }

    #[tokio::test]
    async fn test_mock_output_service_failure() {
        let mock_service = MockOutputService::new().with_failure(true);

        let test_image = create_test_image();
        let result = mock_service.post_to_social_media(test_image).await;

        assert!(result.is_err());
        assert_eq!(mock_service.get_call_count(), 1); // Still counts the attempt
    }

    #[tokio::test]
    async fn test_mock_output_service_multiple_calls() {
        let mock_service = MockOutputService::new();

        let test_image = create_test_image();

        // Make multiple calls
        for _ in 0..3 {
            let result = mock_service.post_to_social_media(test_image.clone()).await;
            assert!(result.is_ok());
        }

        assert_eq!(mock_service.get_call_count(), 3);
    }
}
