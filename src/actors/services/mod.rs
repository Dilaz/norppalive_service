pub mod bluesky;
pub mod generic_actor;
pub mod kafka;
pub mod mastodon;
pub mod twitter;

pub use bluesky::BlueskyActor;
pub use generic_actor::ServiceActor;
pub use kafka::KafkaActor;
pub use mastodon::MastodonActor;
pub use twitter::TwitterActor;

// Type aliases for backwards compatibility during migration
use crate::services::{BlueskyService, KafkaService, MastodonService, TwitterService};

pub type TwitterServiceActor = ServiceActor<TwitterService>;
pub type BlueskyServiceActor = ServiceActor<BlueskyService>;
pub type MastodonServiceActor = ServiceActor<MastodonService>;
pub type KafkaServiceActor = ServiceActor<KafkaService>;
