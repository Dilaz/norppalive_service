mod bluesky;
pub mod generic;
mod kafka;
mod mastodon;
mod twitter;

pub use bluesky::BlueskyService;
pub use generic::ServiceType;
pub use generic::SocialMediaService;
pub use kafka::{DetectionKafkaService, KafkaService};
pub use mastodon::MastodonService;
pub use twitter::TwitterService;
