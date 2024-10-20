use super::{BlueskyService, MastodonService, TwitterService};

pub trait SocialMediaService {
    fn post(&self, message: &str, image_path: &str) -> impl std::future::Future<Output = Result<(), String>> + Send;
}

pub enum ServiceType {
    Twitter(TwitterService),
    Mastodon(MastodonService),
    Bluesky(BlueskyService),
}
