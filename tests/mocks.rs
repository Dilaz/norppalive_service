use std::sync::{Arc, Mutex};
use tracing::info;

use norppalive_service::error::NorppaliveError;
use norppalive_service::messages::{
    GetServiceStatus, ServicePost, ServicePostResult, ServiceStatus,
};
use norppalive_service::services::SocialMediaService;

// Import actix for the mock actors
use actix::prelude::*;
use chrono::Utc;

/// Mock post data structure for testing
#[derive(Debug, Clone)]
pub struct MockPost {
    #[allow(dead_code)] // Field was reported as unused
    pub message: String,
    #[allow(dead_code)] // Field was reported as unused
    pub image_path: String,
    pub service_name: String,
    #[allow(dead_code)] // Field was reported as unused
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// Shared mock data between all mock services
#[derive(Debug, Default)]
pub struct MockData {
    pub posts: Vec<MockPost>,
    pub should_fail: bool,
    pub failure_message: Option<String>,
}

impl MockData {
    #[allow(dead_code)] // Method was reported as unused
    pub fn with_failure(mut self, should_fail: bool, message: Option<String>) -> Self {
        self.should_fail = should_fail;
        self.failure_message = message;
        self
    }

    #[allow(dead_code)] // Method was reported as unused
    pub fn get_posts(&self) -> Vec<MockPost> {
        self.posts.clone()
    }

    pub fn get_posts_for_service(&self, service_name: &str) -> Vec<MockPost> {
        self.posts
            .iter()
            .filter(|post| post.service_name == service_name)
            .cloned()
            .collect()
    }

    pub fn get_post_count(&self) -> usize {
        self.posts.len()
    }

    #[allow(dead_code)] // Method was reported as unused
    pub fn get_post_count_for_service(&self, service_name: &str) -> usize {
        self.posts
            .iter()
            .filter(|post| post.service_name == service_name)
            .count()
    }

    pub fn clear_posts(&mut self) {
        self.posts.clear();
    }
}

/// Mock Twitter service for testing
#[derive(Debug, Clone, Default)]
pub struct MockTwitterService {
    data: Arc<Mutex<MockData>>,
}

impl MockTwitterService {
    pub fn with_shared_data(data: Arc<Mutex<MockData>>) -> Self {
        Self { data }
    }

    #[allow(dead_code)] // Method was reported as unused
    pub fn set_failure(&self, should_fail: bool, message: Option<String>) {
        let mut data = self.data.lock().unwrap();
        data.should_fail = should_fail;
        data.failure_message = message;
    }

    #[allow(dead_code)] // Method was reported as unused
    pub fn get_posts(&self) -> Vec<MockPost> {
        let data = self.data.lock().unwrap();
        data.get_posts_for_service("MockTwitter")
    }

    #[allow(dead_code)] // Method was reported as unused
    pub fn get_post_count(&self) -> usize {
        let data = self.data.lock().unwrap();
        data.get_post_count_for_service("MockTwitter")
    }

    #[allow(dead_code)] // Method was reported as unused
    pub fn clear_posts(&self) {
        let mut data = self.data.lock().unwrap();
        data.posts.retain(|post| post.service_name != "MockTwitter");
    }
}

impl SocialMediaService for MockTwitterService {
    async fn post(&self, message: &str, image_path: &str) -> Result<(), NorppaliveError> {
        let mut data = self.data.lock().unwrap();

        if data.should_fail {
            let error_msg = data
                .failure_message
                .clone()
                .unwrap_or_else(|| "Mock Twitter failure".to_string());
            return Err(NorppaliveError::Other(error_msg));
        }

        let post = MockPost {
            message: message.to_string(),
            image_path: image_path.to_string(),
            service_name: "MockTwitter".to_string(),
            timestamp: chrono::Utc::now(),
        };

        data.posts.push(post);
        info!(
            "Mock: Posted to Twitter - message: {}, image: {}",
            message, image_path
        );
        Ok(())
    }

    fn name(&self) -> &'static str {
        "MockTwitter"
    }
}

/// Mock Mastodon service for testing
#[derive(Debug, Clone, Default)]
pub struct MockMastodonService {
    data: Arc<Mutex<MockData>>,
}

impl MockMastodonService {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_shared_data(data: Arc<Mutex<MockData>>) -> Self {
        Self { data }
    }

    #[allow(dead_code)] // Method was reported as unused
    pub fn set_failure(&self, should_fail: bool, message: Option<String>) {
        let mut data = self.data.lock().unwrap();
        data.should_fail = should_fail;
        data.failure_message = message;
    }

    #[allow(dead_code)] // Method was reported as unused
    pub fn get_posts(&self) -> Vec<MockPost> {
        let data = self.data.lock().unwrap();
        data.get_posts_for_service("MockMastodon")
    }

    #[allow(dead_code)] // Method was reported as unused
    pub fn get_post_count(&self) -> usize {
        let data = self.data.lock().unwrap();
        data.get_post_count_for_service("MockMastodon")
    }

    #[allow(dead_code)] // Method was reported as unused
    pub fn clear_posts(&self) {
        let mut data = self.data.lock().unwrap();
        data.posts
            .retain(|post| post.service_name != "MockMastodon");
    }
}

impl SocialMediaService for MockMastodonService {
    async fn post(&self, message: &str, image_path: &str) -> Result<(), NorppaliveError> {
        let mut data = self.data.lock().unwrap();

        if data.should_fail {
            let error_msg = data
                .failure_message
                .clone()
                .unwrap_or_else(|| "Mock Mastodon failure".to_string());
            return Err(NorppaliveError::Other(error_msg));
        }

        let post = MockPost {
            message: message.to_string(),
            image_path: image_path.to_string(),
            service_name: "MockMastodon".to_string(),
            timestamp: chrono::Utc::now(),
        };

        data.posts.push(post);
        info!(
            "Mock: Posted to Mastodon - message: {}, image: {}",
            message, image_path
        );
        Ok(())
    }

    fn name(&self) -> &'static str {
        "MockMastodon"
    }
}

/// Mock Bluesky service for testing
#[derive(Debug, Clone, Default)]
pub struct MockBlueskyService {
    data: Arc<Mutex<MockData>>,
}

impl MockBlueskyService {
    pub fn with_shared_data(data: Arc<Mutex<MockData>>) -> Self {
        Self { data }
    }

    #[allow(dead_code)] // Method was reported as unused
    pub fn set_failure(&self, should_fail: bool, message: Option<String>) {
        let mut data = self.data.lock().unwrap();
        data.should_fail = should_fail;
        data.failure_message = message;
    }

    #[allow(dead_code)] // Method was reported as unused
    pub fn get_posts(&self) -> Vec<MockPost> {
        let data = self.data.lock().unwrap();
        data.get_posts_for_service("MockBluesky")
    }

    #[allow(dead_code)] // Method was reported as unused
    pub fn get_post_count(&self) -> usize {
        let data = self.data.lock().unwrap();
        data.get_post_count_for_service("MockBluesky")
    }

    #[allow(dead_code)] // Method was reported as unused
    pub fn clear_posts(&self) {
        let mut data = self.data.lock().unwrap();
        data.posts.retain(|post| post.service_name != "MockBluesky");
    }
}

impl SocialMediaService for MockBlueskyService {
    async fn post(&self, message: &str, image_path: &str) -> Result<(), NorppaliveError> {
        let mut data = self.data.lock().unwrap();

        if data.should_fail {
            let error_msg = data
                .failure_message
                .clone()
                .unwrap_or_else(|| "Mock Bluesky failure".to_string());
            return Err(NorppaliveError::Other(error_msg));
        }

        let post = MockPost {
            message: message.to_string(),
            image_path: image_path.to_string(),
            service_name: "MockBluesky".to_string(),
            timestamp: chrono::Utc::now(),
        };

        data.posts.push(post);
        info!(
            "Mock: Posted to Bluesky - message: {}, image: {}",
            message, image_path
        );
        Ok(())
    }

    fn name(&self) -> &'static str {
        "MockBluesky"
    }
}

/// Mock Kafka service for testing
#[derive(Debug, Clone, Default)]
pub struct MockKafkaService {
    data: Arc<Mutex<MockData>>,
}

impl MockKafkaService {
    pub fn with_shared_data(data: Arc<Mutex<MockData>>) -> Self {
        Self { data }
    }

    #[allow(dead_code)] // Method was reported as unused
    pub fn set_failure(&self, should_fail: bool, message: Option<String>) {
        let mut data = self.data.lock().unwrap();
        data.should_fail = should_fail;
        data.failure_message = message;
    }

    #[allow(dead_code)] // Method was reported as unused
    pub fn get_posts(&self) -> Vec<MockPost> {
        let data = self.data.lock().unwrap();
        data.get_posts_for_service("MockKafka")
    }

    #[allow(dead_code)] // Method was reported as unused
    pub fn get_post_count(&self) -> usize {
        let data = self.data.lock().unwrap();
        data.get_post_count_for_service("MockKafka")
    }

    #[allow(dead_code)] // Method was reported as unused
    pub fn clear_posts(&self) {
        let mut data = self.data.lock().unwrap();
        data.posts.retain(|post| post.service_name != "MockKafka");
    }
}

impl SocialMediaService for MockKafkaService {
    async fn post(&self, message: &str, image_path: &str) -> Result<(), NorppaliveError> {
        let mut data = self.data.lock().unwrap();

        if data.should_fail {
            let error_msg = data
                .failure_message
                .clone()
                .unwrap_or_else(|| "Mock Kafka failure".to_string());
            return Err(NorppaliveError::Other(error_msg));
        }

        let post = MockPost {
            message: message.to_string(),
            image_path: image_path.to_string(),
            service_name: "MockKafka".to_string(),
            timestamp: chrono::Utc::now(),
        };

        data.posts.push(post);
        info!(
            "Mock: Posted to Kafka - message: {}, image: {}",
            message, image_path
        );
        Ok(())
    }

    fn name(&self) -> &'static str {
        "MockKafka"
    }
}

/// Mock service manager that creates services with shared state
#[derive(Debug, Clone, Default)]
pub struct MockServiceManager {
    data: Arc<Mutex<MockData>>,
}

impl MockServiceManager {
    pub fn create_twitter_service(&self) -> MockTwitterService {
        MockTwitterService::with_shared_data(self.data.clone())
    }

    pub fn create_mastodon_service(&self) -> MockMastodonService {
        MockMastodonService::with_shared_data(self.data.clone())
    }

    pub fn create_bluesky_service(&self) -> MockBlueskyService {
        MockBlueskyService::with_shared_data(self.data.clone())
    }

    pub fn create_kafka_service(&self) -> MockKafkaService {
        MockKafkaService::with_shared_data(self.data.clone())
    }

    pub fn set_global_failure(&self, should_fail: bool, message: Option<String>) {
        let mut data = self.data.lock().unwrap();
        data.should_fail = should_fail;
        data.failure_message = message;
    }

    #[allow(dead_code)] // Method was reported as unused
    pub fn get_all_posts(&self) -> Vec<MockPost> {
        let data = self.data.lock().unwrap();
        data.get_posts()
    }

    pub fn get_posts_for_service(&self, service_name: &str) -> Vec<MockPost> {
        let data = self.data.lock().unwrap();
        data.get_posts_for_service(service_name)
    }

    pub fn get_total_post_count(&self) -> usize {
        let data = self.data.lock().unwrap();
        data.get_post_count()
    }

    pub fn clear_all_posts(&self) {
        let mut data = self.data.lock().unwrap();
        data.clear_posts();
    }
}

/// Mock Twitter Actor for testing - uses MockTwitterService instead of real TwitterService
#[derive(Default)]
pub struct MockTwitterActor {
    service: MockTwitterService,
    service_status: ServiceStatus,
}

impl MockTwitterActor {
    pub fn new() -> Self {
        Self {
            service: MockTwitterService::default(),
            service_status: ServiceStatus {
                name: "Twitter".to_string(),
                healthy: true,
                ..Default::default()
            },
        }
    }

    #[allow(dead_code)] // Add allow dead code here
    pub fn with_shared_data(data: Arc<Mutex<MockData>>) -> Self {
        Self {
            service: MockTwitterService::with_shared_data(data),
            service_status: ServiceStatus {
                name: "Twitter".to_string(),
                ..Default::default()
            },
        }
    }
}

impl Actor for MockTwitterActor {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        info!("MockTwitterActor started");
    }
}

impl Handler<ServicePost> for MockTwitterActor {
    type Result = ResponseActFuture<Self, Result<ServicePostResult, NorppaliveError>>;

    fn handle(&mut self, msg: ServicePost, _ctx: &mut Self::Context) -> Self::Result {
        info!("MockTwitterActor received post request: {}", msg.message);

        let service = self.service.clone();
        let message = msg.message.clone();
        let image_path = msg.image_path.clone();

        Box::pin(
            async move {
                match service.post(&message, &image_path).await {
                    Ok(_) => Ok(ServicePostResult {
                        success: true,
                        service_name: "Twitter".to_string(),
                        error_message: None,
                        posted_at: Utc::now().timestamp(),
                    }),
                    Err(err) => Ok(ServicePostResult {
                        success: false,
                        service_name: "Twitter".to_string(),
                        error_message: Some(err.to_string()),
                        posted_at: Utc::now().timestamp(),
                    }),
                }
            }
            .into_actor(self)
            .map(|result, actor, _ctx| {
                if let Ok(ref post_result) = result {
                    if post_result.success {
                        actor.service_status.last_post_time = Some(Utc::now().timestamp());
                        actor.service_status.healthy = true;
                    } else {
                        actor.service_status.error_count += 1;
                        actor.service_status.healthy = false;
                    }
                }
                result
            }),
        )
    }
}

impl Handler<GetServiceStatus> for MockTwitterActor {
    type Result = Result<ServiceStatus, NorppaliveError>;

    fn handle(&mut self, _msg: GetServiceStatus, _ctx: &mut Self::Context) -> Self::Result {
        Ok(self.service_status.clone())
    }
}

/// Mock Mastodon Actor for testing
#[derive(Default)]
pub struct MockMastodonActor {
    service: MockMastodonService,
    service_status: ServiceStatus,
}

impl MockMastodonActor {
    #[allow(dead_code)] // Allow dead code as it's used by other test targets
    pub fn new() -> Self {
        Self {
            service: MockMastodonService::new(),
            service_status: ServiceStatus {
                name: "Mastodon".to_string(),
                healthy: true,
                ..Default::default()
            },
        }
    }

    #[allow(dead_code)] // Add allow dead code here
    pub fn with_shared_data(data: Arc<Mutex<MockData>>) -> Self {
        Self {
            service: MockMastodonService::with_shared_data(data),
            service_status: ServiceStatus {
                name: "Mastodon".to_string(),
                ..Default::default()
            },
        }
    }
}

impl Actor for MockMastodonActor {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        info!("MockMastodonActor started");
    }
}

impl Handler<ServicePost> for MockMastodonActor {
    type Result = ResponseActFuture<Self, Result<ServicePostResult, NorppaliveError>>;

    fn handle(&mut self, msg: ServicePost, _ctx: &mut Self::Context) -> Self::Result {
        info!("MockMastodonActor received post request: {}", msg.message);

        let service = self.service.clone();
        let message = msg.message.clone();
        let image_path = msg.image_path.clone();

        Box::pin(
            async move {
                match service.post(&message, &image_path).await {
                    Ok(_) => Ok(ServicePostResult {
                        success: true,
                        service_name: "Mastodon".to_string(),
                        error_message: None,
                        posted_at: Utc::now().timestamp(),
                    }),
                    Err(err) => Ok(ServicePostResult {
                        success: false,
                        service_name: "Mastodon".to_string(),
                        error_message: Some(err.to_string()),
                        posted_at: Utc::now().timestamp(),
                    }),
                }
            }
            .into_actor(self)
            .map(|result, actor, _ctx| {
                if let Ok(ref post_result) = result {
                    if post_result.success {
                        actor.service_status.last_post_time = Some(Utc::now().timestamp());
                        actor.service_status.healthy = true;
                    } else {
                        actor.service_status.error_count += 1;
                        actor.service_status.healthy = false;
                    }
                }
                result
            }),
        )
    }
}

impl Handler<GetServiceStatus> for MockMastodonActor {
    type Result = Result<ServiceStatus, NorppaliveError>;

    fn handle(&mut self, _msg: GetServiceStatus, _ctx: &mut Self::Context) -> Self::Result {
        Ok(self.service_status.clone())
    }
}

/// Mock Bluesky Actor for testing
#[derive(Default)]
pub struct MockBlueskyActor {
    service: MockBlueskyService,
    service_status: ServiceStatus,
}

impl MockBlueskyActor {
    pub fn new() -> Self {
        Self {
            service: MockBlueskyService::default(),
            service_status: ServiceStatus {
                name: "Bluesky".to_string(),
                healthy: true,
                ..Default::default()
            },
        }
    }

    #[allow(dead_code)] // Add allow dead code here
    pub fn with_shared_data(data: Arc<Mutex<MockData>>) -> Self {
        Self {
            service: MockBlueskyService::with_shared_data(data),
            service_status: ServiceStatus {
                name: "Bluesky".to_string(),
                ..Default::default()
            },
        }
    }
}

impl Actor for MockBlueskyActor {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        info!("MockBlueskyActor started");
    }
}

impl Handler<ServicePost> for MockBlueskyActor {
    type Result = ResponseActFuture<Self, Result<ServicePostResult, NorppaliveError>>;

    fn handle(&mut self, msg: ServicePost, _ctx: &mut Self::Context) -> Self::Result {
        info!("MockBlueskyActor received post request: {}", msg.message);

        let service = self.service.clone();
        let message = msg.message.clone();
        let image_path = msg.image_path.clone();

        Box::pin(
            async move {
                match service.post(&message, &image_path).await {
                    Ok(_) => Ok(ServicePostResult {
                        success: true,
                        service_name: "Bluesky".to_string(),
                        error_message: None,
                        posted_at: Utc::now().timestamp(),
                    }),
                    Err(err) => Ok(ServicePostResult {
                        success: false,
                        service_name: "Bluesky".to_string(),
                        error_message: Some(err.to_string()),
                        posted_at: Utc::now().timestamp(),
                    }),
                }
            }
            .into_actor(self)
            .map(|result, actor, _ctx| {
                if let Ok(ref post_result) = result {
                    if post_result.success {
                        actor.service_status.last_post_time = Some(Utc::now().timestamp());
                        actor.service_status.healthy = true;
                    } else {
                        actor.service_status.error_count += 1;
                        actor.service_status.healthy = false;
                    }
                }
                result
            }),
        )
    }
}

impl Handler<GetServiceStatus> for MockBlueskyActor {
    type Result = Result<ServiceStatus, NorppaliveError>;

    fn handle(&mut self, _msg: GetServiceStatus, _ctx: &mut Self::Context) -> Self::Result {
        Ok(self.service_status.clone())
    }
}

/// Mock Kafka Actor for testing
#[derive(Default)]
pub struct MockKafkaActor {
    service: MockKafkaService,
    service_status: ServiceStatus,
}

impl MockKafkaActor {
    #[allow(dead_code)] // Allow dead code as it's used by other test targets
    pub fn new() -> Self {
        Self {
            service: MockKafkaService::default(),
            service_status: ServiceStatus {
                name: "Kafka".to_string(),
                healthy: true,
                ..Default::default()
            },
        }
    }

    #[allow(dead_code)] // Add allow dead code here
    pub fn with_shared_data(data: Arc<Mutex<MockData>>) -> Self {
        Self {
            service: MockKafkaService::with_shared_data(data),
            service_status: ServiceStatus {
                name: "Kafka".to_string(),
                ..Default::default()
            },
        }
    }
}

impl Actor for MockKafkaActor {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        info!("MockKafkaActor started");
    }
}

impl Handler<ServicePost> for MockKafkaActor {
    type Result = ResponseActFuture<Self, Result<ServicePostResult, NorppaliveError>>;

    fn handle(&mut self, msg: ServicePost, _ctx: &mut Self::Context) -> Self::Result {
        info!("MockKafkaActor received post request: {}", msg.message);

        let service = self.service.clone();
        let message = msg.message.clone();
        let image_path = msg.image_path.clone();

        Box::pin(
            async move {
                match service.post(&message, &image_path).await {
                    Ok(_) => Ok(ServicePostResult {
                        success: true,
                        service_name: "Kafka".to_string(),
                        error_message: None,
                        posted_at: Utc::now().timestamp(),
                    }),
                    Err(err) => Ok(ServicePostResult {
                        success: false,
                        service_name: "Kafka".to_string(),
                        error_message: Some(err.to_string()),
                        posted_at: Utc::now().timestamp(),
                    }),
                }
            }
            .into_actor(self)
            .map(|result, actor, _ctx| {
                if let Ok(ref post_result) = result {
                    if post_result.success {
                        actor.service_status.last_post_time = Some(Utc::now().timestamp());
                        actor.service_status.healthy = true;
                    } else {
                        actor.service_status.error_count += 1;
                        actor.service_status.healthy = false;
                    }
                }
                result
            }),
        )
    }
}

impl Handler<GetServiceStatus> for MockKafkaActor {
    type Result = Result<ServiceStatus, NorppaliveError>;

    fn handle(&mut self, _msg: GetServiceStatus, _ctx: &mut Self::Context) -> Self::Result {
        Ok(self.service_status.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use norppalive_service::messages::GetServiceStatus;

    #[tokio::test]
    async fn test_mock_service_manager() {
        let manager = MockServiceManager::default();
        let twitter = manager.create_twitter_service();
        let mastodon = manager.create_mastodon_service();

        twitter.post("Test 1", "/path/to/image1.jpg").await.unwrap();
        mastodon
            .post("Test 2", "/path/to/image2.jpg")
            .await
            .unwrap();

        assert_eq!(manager.get_total_post_count(), 2);
        assert_eq!(manager.get_posts_for_service("MockTwitter").len(), 1);
        assert_eq!(manager.get_posts_for_service("MockMastodon").len(), 1);

        manager.clear_all_posts();
        assert_eq!(manager.get_total_post_count(), 0);
    }

    #[tokio::test]
    async fn test_mock_service_manager_global_failure() {
        let manager = MockServiceManager::default();
        manager.set_global_failure(true, Some("Network error".to_string()));

        let twitter = manager.create_twitter_service();
        let result = twitter.post("Test", "/path/to/image.jpg").await;

        assert!(result.is_err());
        assert_eq!(manager.get_total_post_count(), 0);
    }

    #[tokio::test]
    async fn test_all_mock_services() {
        let manager = MockServiceManager::default();

        let twitter = manager.create_twitter_service();
        let mastodon = manager.create_mastodon_service();
        let bluesky = manager.create_bluesky_service();
        let kafka = manager.create_kafka_service();

        let message = "Test message";
        let image_path = "/test/image.jpg";

        twitter.post(message, image_path).await.unwrap();
        mastodon.post(message, image_path).await.unwrap();
        bluesky.post(message, image_path).await.unwrap();
        kafka.post(message, image_path).await.unwrap();

        assert_eq!(manager.get_total_post_count(), 4);
        assert_eq!(twitter.name(), "MockTwitter");
        assert_eq!(mastodon.name(), "MockMastodon");
        assert_eq!(bluesky.name(), "MockBluesky");
        assert_eq!(kafka.name(), "MockKafka");
    }

    #[actix::test]
    async fn test_twitter_actor_creation() {
        let actor = MockTwitterActor::new();
        let actor_addr = actor.start();
        let status = actor_addr.send(GetServiceStatus).await.unwrap().unwrap();

        assert_eq!(status.name, "Twitter");
        assert!(status.healthy);
        assert_eq!(status.error_count, 0);
        assert!(!status.rate_limited);
    }

    #[actix::test]
    async fn test_twitter_actor_startup() {
        let actor = MockTwitterActor::new().start();

        let status = actor.send(GetServiceStatus).await.unwrap().unwrap();
        assert_eq!(status.name, "Twitter");
        assert!(status.healthy);
        assert_eq!(status.error_count, 0);
        assert!(!status.rate_limited);
    }

    #[actix::test]
    async fn test_twitter_get_service_status() {
        let actor = MockTwitterActor::new().start();

        let status = actor.send(GetServiceStatus).await.unwrap().unwrap();

        assert_eq!(status.name, "Twitter");
        assert!(status.healthy);
        assert!(status.last_post_time.is_none());
        assert_eq!(status.error_count, 0);
        assert!(!status.rate_limited);
    }

    #[actix::test]
    async fn test_mastodon_actor_startup() {
        let actor = MockMastodonActor::new().start();

        let status = actor.send(GetServiceStatus).await.unwrap().unwrap();
        assert_eq!(status.name, "Mastodon");
        assert!(status.healthy);
        assert_eq!(status.error_count, 0);
        assert!(!status.rate_limited);
    }

    #[actix::test]
    async fn test_mastodon_get_service_status() {
        let actor = MockMastodonActor::new().start();

        let status = actor.send(GetServiceStatus).await.unwrap().unwrap();

        assert_eq!(status.name, "Mastodon");
        assert!(status.healthy);
        assert!(status.last_post_time.is_none());
        assert_eq!(status.error_count, 0);
        assert!(!status.rate_limited);
    }

    #[actix::test]
    async fn test_bluesky_actor_startup() {
        let actor = MockBlueskyActor::new().start();

        let status = actor.send(GetServiceStatus).await.unwrap().unwrap();
        assert_eq!(status.name, "Bluesky");
        assert!(status.healthy);
        assert_eq!(status.error_count, 0);
        assert!(!status.rate_limited);
    }

    #[actix::test]
    async fn test_bluesky_get_service_status() {
        let actor = MockBlueskyActor::new().start();

        let status = actor.send(GetServiceStatus).await.unwrap().unwrap();

        assert_eq!(status.name, "Bluesky");
        assert!(status.healthy);
        assert!(status.last_post_time.is_none());
        assert_eq!(status.error_count, 0);
        assert!(!status.rate_limited);
    }

    #[actix::test]
    async fn test_kafka_actor_startup() {
        let actor = MockKafkaActor::new().start();

        let status = actor.send(GetServiceStatus).await.unwrap().unwrap();
        assert_eq!(status.name, "Kafka");
        assert!(status.healthy);
        assert_eq!(status.error_count, 0);
        assert!(!status.rate_limited);
    }

    #[actix::test]
    async fn test_kafka_get_service_status() {
        let actor = MockKafkaActor::new().start();

        let status = actor.send(GetServiceStatus).await.unwrap().unwrap();

        assert_eq!(status.name, "Kafka");
        assert!(status.healthy);
        assert!(status.last_post_time.is_none());
        assert_eq!(status.error_count, 0);
        assert!(!status.rate_limited);
    }

    #[actix::test]
    async fn test_all_service_actors_basic_functionality() {
        let _manager = MockServiceManager::default();

        let twitter_actor = MockTwitterActor::new().start();
        let mastodon_actor = MockMastodonActor::new().start();
        let bluesky_actor = MockBlueskyActor::new().start();
        let kafka_actor = MockKafkaActor::new().start();

        let twitter_status = twitter_actor.send(GetServiceStatus).await.unwrap().unwrap();
        let mastodon_status = mastodon_actor
            .send(GetServiceStatus)
            .await
            .unwrap()
            .unwrap();
        let bluesky_status = bluesky_actor.send(GetServiceStatus).await.unwrap().unwrap();
        let kafka_status = kafka_actor.send(GetServiceStatus).await.unwrap().unwrap();

        assert_eq!(twitter_status.name, "Twitter");
        assert!(twitter_status.healthy);

        assert_eq!(mastodon_status.name, "Mastodon");
        assert!(mastodon_status.healthy);

        assert_eq!(bluesky_status.name, "Bluesky");
        assert!(bluesky_status.healthy);

        assert_eq!(kafka_status.name, "Kafka");
        assert!(kafka_status.healthy);
    }
}
