use actix::prelude::*;
use std::io::Write;

use norppalive_service::actors::services::TwitterActor; // Import the real TwitterActor
use norppalive_service::actors::services::{BlueskyActor, KafkaActor, MastodonActor};
use norppalive_service::messages::GetServiceStatus; // Added imports

#[cfg(feature = "test-utils")] // Guard imports needed for tests using mocks
use norppalive_service::services::{
    generic::test_mocks::MockMockSocialMedia, // The mock itself
    ServiceType,                              // To wrap the mock
};

// Import the mock actors instead of production actors
// mod mocks; // Removed - this will require refactoring these tests to use mockall mocks
// use mocks::{MockBlueskyActor, MockKafkaActor, MockMastodonActor, MockTwitterActor}; // Removed

#[allow(dead_code)]
fn create_test_image_file() -> Result<tempfile::NamedTempFile, std::io::Error> {
    let mut file = tempfile::NamedTempFile::new()?;
    file.write_all(b"fake image data for testing")?;
    Ok(file)
}

#[actix::test]
#[cfg_attr(
    not(feature = "test-utils"),
    ignore = "requires test-utils feature for mocks"
)] // Ignore if feature not set
async fn test_twitter_actor_creation() {
    #[cfg(feature = "test-utils")] // Entire test logic depends on this feature
    {
        let mut mock_service = MockMockSocialMedia::new();
        mock_service.expect_name().times(1).returning(|| "Twitter"); // The actor's new() method calls service.name()

        // When GetServiceStatus is handled, it clones service_status which was initialized with name.
        // If other methods of the service were called by the actor, we'd mock them here.

        let service_type_mock: ServiceType = mock_service.into(); // Convert mock to ServiceType
        let twitter_actor = TwitterActor::new(service_type_mock); // Inject mocked service

        let actor_addr = twitter_actor.start();
        let status_result = actor_addr.send(GetServiceStatus).await;

        assert!(status_result.is_ok(), "GetServiceStatus send failed");
        let status = status_result
            .unwrap()
            .expect("GetServiceStatus handler failed");

        assert_eq!(status.name, "Twitter");
        assert!(status.healthy);
        assert_eq!(status.error_count, 0);
        assert!(!status.rate_limited);
    }
    #[cfg(not(feature = "test-utils"))]
    {
        // If mocks aren't available, this test can't run as intended.
        // Alternatively, one could provide a real service, but that changes the test's nature.
        println!("Skipping test_twitter_actor_creation as test-utils feature is not enabled.");
    }
}

#[actix::test]
#[cfg_attr(
    not(feature = "test-utils"),
    ignore = "requires test-utils feature for mocks"
)]
async fn test_twitter_actor_startup() {
    #[cfg(feature = "test-utils")]
    {
        let mut mock_service = MockMockSocialMedia::new();
        mock_service.expect_name().times(1).returning(|| "Twitter");
        let service_type_mock: ServiceType = mock_service.into();
        let actor = TwitterActor::new(service_type_mock).start();

        // Test getting service status
        let status_result = actor.send(GetServiceStatus).await;
        assert!(status_result.is_ok(), "GetServiceStatus send failed");
        let status = status_result
            .unwrap()
            .expect("GetServiceStatus handler failed");
        assert_eq!(status.name, "Twitter");
        assert!(status.healthy);
        assert_eq!(status.error_count, 0);
        assert!(!status.rate_limited);
    }
    #[cfg(not(feature = "test-utils"))]
    {
        println!("Skipping test_twitter_actor_startup as test-utils feature is not enabled.");
    }
}

#[actix::test]
#[cfg_attr(
    not(feature = "test-utils"),
    ignore = "requires test-utils feature for mocks"
)]
async fn test_twitter_get_service_status() {
    #[cfg(feature = "test-utils")]
    {
        let mut mock_service = MockMockSocialMedia::new();
        mock_service.expect_name().times(1).returning(|| "Twitter");
        let service_type_mock: ServiceType = mock_service.into();
        let actor = TwitterActor::new(service_type_mock).start();

        let status_result = actor.send(GetServiceStatus).await;
        assert!(status_result.is_ok(), "GetServiceStatus send failed");
        let status = status_result
            .unwrap()
            .expect("GetServiceStatus handler failed");

        // Verify status structure
        assert_eq!(status.name, "Twitter");
        assert!(status.healthy);
        assert!(status.last_post_time.is_none());
        assert_eq!(status.error_count, 0);
        assert!(!status.rate_limited);
    }
    #[cfg(not(feature = "test-utils"))]
    {
        println!("Skipping test_twitter_get_service_status as test-utils feature is not enabled.");
    }
}

#[actix::test]
#[cfg_attr(
    not(feature = "test-utils"),
    ignore = "requires test-utils feature for mocks"
)]
async fn test_twitter_concurrent_status_requests() {
    #[cfg(feature = "test-utils")]
    {
        let mut mock_service = MockMockSocialMedia::new();
        // Name is called once per actor instantiation.
        // If the actor was cloned and started multiple times, this would be different.
        // For this test, one actor instance is created.
        mock_service.expect_name().times(1).returning(|| "Twitter");

        let service_type_mock: ServiceType = mock_service.into();
        let actor_addr = TwitterActor::new(service_type_mock).start();

        // Send multiple status requests concurrently
        let results = futures::future::join_all(vec![
            actor_addr.send(GetServiceStatus),
            actor_addr.send(GetServiceStatus),
            actor_addr.send(GetServiceStatus),
        ])
        .await;

        // All should succeed
        for result in results {
            assert!(result.is_ok(), "Concurrent GetServiceStatus send failed");
            let status_option = result.unwrap();
            assert!(
                status_option.is_ok(),
                "Concurrent GetServiceStatus handler failed"
            );
            let status = status_option.unwrap();
            assert_eq!(status.name, "Twitter");
            assert!(status.healthy);
        }
    }
    #[cfg(not(feature = "test-utils"))]
    {
        println!("Skipping test_twitter_concurrent_status_requests as test-utils feature is not enabled.");
    }
}

#[actix::test]
#[cfg_attr(
    not(feature = "test-utils"),
    ignore = "requires test-utils feature for mocks"
)]
async fn test_mastodon_actor_creation() {
    #[cfg(feature = "test-utils")]
    {
        let mut mock_service = MockMockSocialMedia::new();
        mock_service.expect_name().times(1).returning(|| "Mastodon");
        let service_type_mock: ServiceType = mock_service.into();
        let actor = MastodonActor::new(service_type_mock).start(); // Use real MastodonActor

        let status_result = actor.send(GetServiceStatus).await;
        assert!(status_result.is_ok(), "GetServiceStatus send failed");
        let status = status_result
            .unwrap()
            .expect("GetServiceStatus handler failed");

        assert_eq!(status.name, "Mastodon");
        assert!(status.healthy);
        assert_eq!(status.error_count, 0);
        assert!(!status.rate_limited);
    }
    #[cfg(not(feature = "test-utils"))]
    {
        println!("Skipping test_mastodon_actor_creation as test-utils feature is not enabled.");
    }
}

#[actix::test]
#[cfg_attr(
    not(feature = "test-utils"),
    ignore = "requires test-utils feature for mocks"
)]
async fn test_bluesky_actor_creation() {
    #[cfg(feature = "test-utils")]
    {
        let mut mock_service = MockMockSocialMedia::new();
        mock_service.expect_name().times(1).returning(|| "Bluesky");
        let service_type_mock: ServiceType = mock_service.into();
        let actor = BlueskyActor::new(service_type_mock).start(); // Use real BlueskyActor

        let status_result = actor.send(GetServiceStatus).await;
        assert!(status_result.is_ok(), "GetServiceStatus send failed");
        let status = status_result
            .unwrap()
            .expect("GetServiceStatus handler failed");

        assert_eq!(status.name, "Bluesky");
        assert!(status.healthy);
        assert_eq!(status.error_count, 0);
        assert!(!status.rate_limited);
    }
    #[cfg(not(feature = "test-utils"))]
    {
        println!("Skipping test_bluesky_actor_creation as test-utils feature is not enabled.");
    }
}

#[actix::test]
#[cfg_attr(
    not(feature = "test-utils"),
    ignore = "requires test-utils feature for mocks"
)]
async fn test_kafka_actor_creation() {
    #[cfg(feature = "test-utils")]
    {
        let mut mock_service = MockMockSocialMedia::new();
        mock_service.expect_name().times(1).returning(|| "Kafka");
        let service_type_mock: ServiceType = mock_service.into();
        let actor = KafkaActor::new(service_type_mock).start(); // Use real KafkaActor

        let status_result = actor.send(GetServiceStatus).await;
        assert!(status_result.is_ok(), "GetServiceStatus send failed");
        let status = status_result
            .unwrap()
            .expect("GetServiceStatus handler failed");

        assert_eq!(status.name, "Kafka");
        assert!(status.healthy);
        assert_eq!(status.error_count, 0);
        assert!(!status.rate_limited);
    }
    #[cfg(not(feature = "test-utils"))]
    {
        println!("Skipping test_kafka_actor_creation as test-utils feature is not enabled.");
    }
}

#[actix::test]
#[cfg_attr(
    not(feature = "test-utils"),
    ignore = "requires test-utils feature for mocks"
)]
async fn test_all_service_actors_basic_functionality() {
    #[cfg(feature = "test-utils")]
    {
        // Twitter
        let mut mock_twitter_service = MockMockSocialMedia::new();
        mock_twitter_service
            .expect_name()
            .times(1)
            .returning(|| "Twitter");
        let twitter_actor = TwitterActor::new(mock_twitter_service.into()).start();

        // Mastodon
        let mut mock_mastodon_service = MockMockSocialMedia::new();
        mock_mastodon_service
            .expect_name()
            .times(1)
            .returning(|| "Mastodon");
        let mastodon_actor = MastodonActor::new(mock_mastodon_service.into()).start();

        // Bluesky
        let mut mock_bluesky_service = MockMockSocialMedia::new();
        mock_bluesky_service
            .expect_name()
            .times(1)
            .returning(|| "Bluesky");
        let bluesky_actor = BlueskyActor::new(mock_bluesky_service.into()).start();

        // Kafka
        let mut mock_kafka_service = MockMockSocialMedia::new();
        mock_kafka_service
            .expect_name()
            .times(1)
            .returning(|| "Kafka");
        let kafka_actor = KafkaActor::new(mock_kafka_service.into()).start();

        // Test that all actors are responsive
        let twitter_status_res = twitter_actor.send(GetServiceStatus).await;
        let mastodon_status_res = mastodon_actor.send(GetServiceStatus).await;
        let bluesky_status_res = bluesky_actor.send(GetServiceStatus).await;
        let kafka_status_res = kafka_actor.send(GetServiceStatus).await;

        // Verify each service status
        let twitter_status = twitter_status_res
            .expect("Twitter GetServiceStatus send failed")
            .expect("Twitter GetServiceStatus handler failed");
        assert_eq!(twitter_status.name, "Twitter");
        assert!(twitter_status.healthy);

        let mastodon_status = mastodon_status_res
            .expect("Mastodon GetServiceStatus send failed")
            .expect("Mastodon GetServiceStatus handler failed");
        assert_eq!(mastodon_status.name, "Mastodon");
        assert!(mastodon_status.healthy);

        let bluesky_status = bluesky_status_res
            .expect("Bluesky GetServiceStatus send failed")
            .expect("Bluesky GetServiceStatus handler failed");
        assert_eq!(bluesky_status.name, "Bluesky");
        assert!(bluesky_status.healthy);

        let kafka_status = kafka_status_res
            .expect("Kafka GetServiceStatus send failed")
            .expect("Kafka GetServiceStatus handler failed");
        assert_eq!(kafka_status.name, "Kafka");
        assert!(kafka_status.healthy);
    }
    #[cfg(not(feature = "test-utils"))]
    {
        println!("Skipping test_all_service_actors_basic_functionality as test-utils feature is not enabled.");
    }
}
