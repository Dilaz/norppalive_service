use actix::prelude::*;
use std::io::Write;
use tempfile;

use norppalive_service::messages::GetServiceStatus;

// Import the mock actors instead of production actors
mod mocks;
use mocks::{MockBlueskyActor, MockKafkaActor, MockMastodonActor, MockTwitterActor};

fn create_test_image_file() -> Result<tempfile::NamedTempFile, std::io::Error> {
    let mut file = tempfile::NamedTempFile::new()?;
    file.write_all(b"fake image data for testing")?;
    Ok(file)
}

#[actix::test]
async fn test_twitter_actor_creation() {
    let actor = MockTwitterActor::new();
    // We can't directly access fields anymore since they're private
    // So we test behavior instead
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

    // Test getting service status
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

    // Verify status structure
    assert_eq!(status.name, "Twitter");
    assert!(status.healthy);
    assert!(status.last_post_time.is_none());
    assert_eq!(status.error_count, 0);
    assert!(!status.rate_limited);
}

#[actix::test]
async fn test_twitter_concurrent_status_requests() {
    let actor = MockTwitterActor::new().start();

    // Send multiple status requests concurrently
    let results = futures::future::join_all(vec![
        actor.send(GetServiceStatus),
        actor.send(GetServiceStatus),
        actor.send(GetServiceStatus),
    ])
    .await;

    // All should succeed
    for result in results {
        let status = result.unwrap().unwrap();
        assert_eq!(status.name, "Twitter");
        assert!(status.healthy);
    }
}

#[actix::test]
async fn test_mastodon_actor_creation() {
    let actor = MockMastodonActor::new().start();
    let status = actor.send(GetServiceStatus).await.unwrap().unwrap();

    assert_eq!(status.name, "Mastodon");
    assert!(status.healthy);
    assert_eq!(status.error_count, 0);
    assert!(!status.rate_limited);
}

#[actix::test]
async fn test_bluesky_actor_creation() {
    let actor = MockBlueskyActor::new().start();
    let status = actor.send(GetServiceStatus).await.unwrap().unwrap();

    assert_eq!(status.name, "Bluesky");
    assert!(status.healthy);
    assert_eq!(status.error_count, 0);
    assert!(!status.rate_limited);
}

#[actix::test]
async fn test_kafka_actor_creation() {
    let actor = MockKafkaActor::new().start();
    let status = actor.send(GetServiceStatus).await.unwrap().unwrap();

    assert_eq!(status.name, "Kafka");
    assert!(status.healthy);
    assert_eq!(status.error_count, 0);
    assert!(!status.rate_limited);
}

#[actix::test]
async fn test_all_service_actors_basic_functionality() {
    // Create all service actors
    let twitter_actor = MockTwitterActor::new().start();
    let mastodon_actor = MockMastodonActor::new().start();
    let bluesky_actor = MockBlueskyActor::new().start();
    let kafka_actor = MockKafkaActor::new().start();

    // Test that all actors are responsive
    let twitter_status = twitter_actor.send(GetServiceStatus).await.unwrap().unwrap();
    let mastodon_status = mastodon_actor
        .send(GetServiceStatus)
        .await
        .unwrap()
        .unwrap();
    let bluesky_status = bluesky_actor.send(GetServiceStatus).await.unwrap().unwrap();
    let kafka_status = kafka_actor.send(GetServiceStatus).await.unwrap().unwrap();

    // Verify each service status
    assert_eq!(twitter_status.name, "Twitter");
    assert!(twitter_status.healthy);

    assert_eq!(mastodon_status.name, "Mastodon");
    assert!(mastodon_status.healthy);

    assert_eq!(bluesky_status.name, "Bluesky");
    assert!(bluesky_status.healthy);

    assert_eq!(kafka_status.name, "Kafka");
    assert!(kafka_status.healthy);
}
