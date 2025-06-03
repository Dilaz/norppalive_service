use async_trait::async_trait;
use enum_dispatch::enum_dispatch;
// #[cfg(test)] // Removed as mock is now in tests/mocks.rs
// use mockall::automock; // Removed

use crate::error::NorppaliveError;

use super::{BlueskyService, KafkaService, MastodonService, TwitterService};

#[cfg(feature = "test-utils")]
use crate::services::generic::test_mocks::MockMockSocialMedia; // Ensure MockMockSocialMedia is in scope
#[cfg(feature = "test-utils")]
use std::sync::Arc; // For Arc<MockMockSocialMedia>

// #[cfg_attr(test, automock)] // Removed
#[async_trait]
#[enum_dispatch(ServiceType)]
pub trait SocialMediaService {
    async fn post(&self, message: &str, image_path: &str) -> Result<(), NorppaliveError>;

    fn name(&self) -> &'static str;
    // Default implementation can be restored if desired, or kept off.
    // For now, keeping it off as concrete types implement it, and mocks will define it.
}

#[enum_dispatch]
#[derive(Clone)]
pub enum ServiceType {
    TwitterService(TwitterService),
    MastodonService(MastodonService),
    BlueskyService(BlueskyService),
    KafkaService(KafkaService),
    #[cfg(feature = "test-utils")]
    MockedService(Arc<MockMockSocialMedia>),
}

// This From<MockMockSocialMedia> should be fine as enum_dispatch won't create it.
#[cfg(feature = "test-utils")]
impl From<MockMockSocialMedia> for ServiceType {
    fn from(mock_service: MockMockSocialMedia) -> Self {
        ServiceType::MockedService(Arc::new(mock_service))
    }
}

// ADDED: Implement SocialMediaService for Arc<MockMockSocialMedia>
#[cfg(feature = "test-utils")]
#[async_trait]
impl SocialMediaService for Arc<MockMockSocialMedia> {
    async fn post(&self, message: &str, image_path: &str) -> Result<(), NorppaliveError> {
        // Dereference Arc to get &MockMockSocialMedia, then call its method.
        // MockMockSocialMedia itself implements SocialMediaService.
        (**self).post(message, image_path).await
    }

    fn name(&self) -> &'static str {
        (**self).name()
    }
}

#[cfg(feature = "test-utils")]
pub mod test_mocks {
    // Made public
    use super::SocialMediaService;
    use crate::error::NorppaliveError;
    use async_trait::async_trait;
    use mockall::mock; // Use super to get the trait

    mock! {
        // Ensure this MockMockSocialMedia generated struct is Clone.
        // This is needed if we ever try to clone ServiceType::MockedService(mock) directly
        // without the Arc, or if other code tries to clone MockMockSocialMedia.
        // For ServiceType::MockedService(Arc<MockMockSocialMedia>), Arc handles the clone.
        // But good to keep it, as it caused issues before when ServiceType directly held MockMockSocialMedia.
        #[derive(Clone)]
        pub MockSocialMedia {
            // Methods will be mirrored from SocialMediaService trait
        }

        // This block defines how the struct MockSocialMedia (which becomes MockMockSocialMedia)
        // implements the SocialMediaService trait.
        #[async_trait]
        impl SocialMediaService for MockSocialMedia { // MockSocialMedia, not MockMockSocialMedia here
            async fn post(&self, message: &str, image_path: &str) -> Result<(), NorppaliveError>;
            fn name(&self) -> &'static str;
        }
    }
}
