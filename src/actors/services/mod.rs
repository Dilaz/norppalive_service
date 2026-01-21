pub mod generic_actor;

pub use generic_actor::ServiceActor;

// Type aliases for convenience
use crate::services::{BlueskyService, KafkaService, MastodonService, TwitterService};

pub type TwitterServiceActor = ServiceActor<TwitterService>;
pub type BlueskyServiceActor = ServiceActor<BlueskyService>;
pub type MastodonServiceActor = ServiceActor<MastodonService>;
pub type KafkaServiceActor = ServiceActor<KafkaService>;
