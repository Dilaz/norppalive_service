use enum_dispatch::enum_dispatch;

use crate::error::NorppaliveError;

use super::{BlueskyService, MastodonService, TwitterService, KafkaService};

#[enum_dispatch(ServiceType)]
pub trait SocialMediaService {
    #[allow(async_fn_in_trait)]
    async fn post(&self, message: &str, image_path: &str) -> Result<(), NorppaliveError>;

    fn name(&self) -> &'static str {
        "Generic"
    }
}

#[enum_dispatch]
pub enum ServiceType {
    TwitterService,
    MastodonService,
    BlueskyService,
    KafkaService,
}
