use crate::{
    config::{Service, CONFIG},
    error::NorppaliveError,
    services::{
        BlueskyService, KafkaService, MastodonService, ServiceType, SocialMediaService,
        TwitterService,
    },
};
use chrono::Utc;
use image::DynamicImage;
use std::future::Future;
use std::pin::Pin;
use tracing::info;
// use rand::prelude::*; // Commented out as it's unused

// Trait for output service abstraction
pub trait OutputServiceTrait: Send + Sync {
    fn post_to_social_media(
        &self,
        image: DynamicImage,
    ) -> Pin<Box<dyn Future<Output = Result<(), NorppaliveError>> + Send>>;
}

// Mock implementation for testing
#[cfg(any(test, feature = "test-utils"))]
pub struct MockOutputService {
    pub should_fail: bool,
    pub call_count: std::sync::Arc<std::sync::Mutex<u32>>,
}

#[cfg(any(test, feature = "test-utils"))]
impl Default for MockOutputService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(any(test, feature = "test-utils"))]
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

#[cfg(any(test, feature = "test-utils"))]
impl OutputServiceTrait for MockOutputService {
    #[allow(clippy::manual_async_fn)]
    fn post_to_social_media(
        &self,
        _image: DynamicImage,
    ) -> Pin<Box<dyn Future<Output = Result<(), NorppaliveError>> + Send>> {
        let should_fail = self.should_fail;
        let call_count = self.call_count.clone();
        Box::pin(async move {
            {
                let mut count = call_count.lock().unwrap();
                *count += 1;
            }

            if should_fail {
                return Err(NorppaliveError::Other("Mock posting failure".to_string()));
            }

            info!("Mock: Posted to social media successfully");
            Ok(())
        })
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
    ) -> Pin<Box<dyn Future<Output = Result<(), NorppaliveError>> + Send>> {
        let services = self.output_services.clone();
        Box::pin(async move {
            let timestamp = Utc::now().format("%Y%m%d_%H%M%S").to_string();
            let temp_dir = tempfile::tempdir().map_err(|e| {
                NorppaliveError::Other(format!("Failed to create temporary directory: {}", e))
            })?;
            let image_filename = format!("norppis_{}.jpg", timestamp);
            let image_path = temp_dir.path().join(&image_filename);

            image.save(&image_path).map_err(NorppaliveError::from)?;

            info!(
                "Posting to social media services, image saved at: {:?}",
                image_path
            );

            let mut errors = Vec::new();
            // Get a random message from config, or use a default if none are available
            let base_message = CONFIG
                .output
                .get_random_message()
                .cloned()
                .unwrap_or_else(|| {
                    tracing::warn!("No messages configured in config.toml, using default message.");
                    "Rare Saimaa ringed seal detected! #norppalive".to_string()
                });

            let message = if CONFIG.output.replace_hashtags {
                // If replace_hashtags is true, replace '#' with 'hashtag-' in the base_message
                base_message.replace("#", "hashtag-")
            } else {
                // Otherwise, use the message as is (assuming it contains its own hashtags and links)
                base_message
            };

            for service in services {
                info!("Posting to service: {}", service.name());
                match service
                    .post(&message, image_path.to_str().unwrap_or_default())
                    .await
                {
                    Ok(_) => info!("Successfully posted to {}", service.name()),
                    Err(e) => {
                        let error_msg = format!("Failed to post to {}: {:?}", service.name(), e);
                        info!("{}", error_msg);
                        errors.push(error_msg);
                    }
                }
            }

            if errors.is_empty() {
                Ok(())
            } else {
                Err(NorppaliveError::Other(format!(
                    "One or more services failed to post: {}",
                    errors.join(", ")
                )))
            }
        })
    }
}

impl Default for OutputService {
    fn default() -> Self {
        let mut output_services: Vec<ServiceType> = vec![];

        for service_config in &CONFIG.output.services {
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
        Self { output_services }
    }
}

impl OutputService {
    pub async fn post_to_social_media(&self, image: DynamicImage) -> Result<(), NorppaliveError> {
        <Self as OutputServiceTrait>::post_to_social_media(self, image).await
    }

    pub fn get_services(&self) -> &Vec<ServiceType> {
        &self.output_services
    }

    pub fn with_services(services: Vec<ServiceType>) -> Self {
        Self {
            output_services: services,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, RgbImage};
    // use crate::services::generic::test_mocks::MockMockSocialMedia; // Commented out

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
        assert_eq!(mock_service.get_call_count(), 1);
    }

    #[tokio::test]
    async fn test_mock_output_service_multiple_calls() {
        let mock_service = MockOutputService::new();

        let test_image = create_test_image();

        for _ in 0..3 {
            let result = mock_service.post_to_social_media(test_image.clone()).await;
            assert!(result.is_ok());
        }

        assert_eq!(mock_service.get_call_count(), 3);
    }

    // New test to verify OutputService with actual (mocked) ServiceType instances
    // #[tokio::test] // Commented out
    // async fn test_output_service_with_mocked_social_media_services() { // Commented out
    // ... entire function body commented out ...
    // } // Commented out
}
