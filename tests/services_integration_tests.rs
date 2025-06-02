use std::fs;

use norppalive_service::services::SocialMediaService;

// Import mocks directly
mod mocks;
use mocks::*;

fn create_test_image_file() -> Result<tempfile::NamedTempFile, std::io::Error> {
    let temp_file = tempfile::NamedTempFile::new()?;
    let test_image_data = b"fake image data for testing purposes";
    fs::write(temp_file.path(), test_image_data)?;
    Ok(temp_file)
}

#[tokio::test]
async fn test_mock_service_manager_integration() {
    // Create a shared service manager for all services
    let manager = MockServiceManager::default();

    // Create all service instances from the manager
    let twitter = manager.create_twitter_service();
    let mastodon = manager.create_mastodon_service();
    let bluesky = manager.create_bluesky_service();
    let kafka = manager.create_kafka_service();

    // Create test image
    let temp_file = create_test_image_file().unwrap();
    let image_path = temp_file.path().to_string_lossy().to_string();

    // Post to all services
    let message = "Rare Saimaa ringed seal detected! 🦭";

    twitter.post(message, &image_path).await.unwrap();
    mastodon.post(message, &image_path).await.unwrap();
    bluesky.post(message, &image_path).await.unwrap();
    kafka.post(message, &image_path).await.unwrap();

    // Verify all posts were recorded
    assert_eq!(manager.get_total_post_count(), 4);

    // Verify each service has one post
    assert_eq!(manager.get_posts_for_service("MockTwitter").len(), 1);
    assert_eq!(manager.get_posts_for_service("MockMastodon").len(), 1);
    assert_eq!(manager.get_posts_for_service("MockBluesky").len(), 1);
    assert_eq!(manager.get_posts_for_service("MockKafka").len(), 1);

    // Verify content of posts
    let twitter_posts = manager.get_posts_for_service("MockTwitter");
    assert_eq!(twitter_posts[0].message, message);
    assert_eq!(twitter_posts[0].image_path, image_path);

    let mastodon_posts = manager.get_posts_for_service("MockMastodon");
    assert_eq!(mastodon_posts[0].message, message);

    // Test clearing posts
    manager.clear_all_posts();
    assert_eq!(manager.get_total_post_count(), 0);
}

#[tokio::test]
async fn test_mock_service_failure_scenarios() {
    let manager = MockServiceManager::default();

    // Set up failure for all services
    manager.set_global_failure(true, Some("Network error simulation".to_string()));

    let twitter = manager.create_twitter_service();
    let mastodon = manager.create_mastodon_service();

    let temp_file = create_test_image_file().unwrap();
    let image_path = temp_file.path().to_string_lossy().to_string();

    // All posts should fail
    let twitter_result = twitter.post("Test message", &image_path).await;
    let mastodon_result = mastodon.post("Test message", &image_path).await;

    assert!(twitter_result.is_err());
    assert!(mastodon_result.is_err());

    // Verify no posts were recorded on failure
    assert_eq!(manager.get_total_post_count(), 0);

    // Reset failure and test recovery
    manager.set_global_failure(false, None);

    let recovery_result = twitter.post("Recovery test", &image_path).await;
    assert!(recovery_result.is_ok());
    assert_eq!(manager.get_total_post_count(), 1);
}

#[tokio::test]
async fn test_individual_service_configurations() {
    // Test different services with different configurations
    let twitter = MockTwitterService::default();
    let mastodon = MockMastodonService::default();

    // Configure Twitter to fail, Mastodon to succeed
    twitter.set_failure(true, Some("Twitter API rate limit".to_string()));

    let temp_file = create_test_image_file().unwrap();
    let image_path = temp_file.path().to_string_lossy().to_string();

    let twitter_result = twitter.post("Test message", &image_path).await;
    let mastodon_result = mastodon.post("Test message", &image_path).await;

    assert!(twitter_result.is_err());
    assert!(mastodon_result.is_ok());

    // Verify only successful post was recorded
    assert_eq!(twitter.get_post_count(), 0);
    assert_eq!(mastodon.get_post_count(), 1);
}

#[tokio::test]
async fn test_concurrent_mock_service_access() {
    use tokio::task;

    let manager = MockServiceManager::default();
    let temp_file = create_test_image_file().unwrap();
    let image_path = temp_file.path().to_string_lossy().to_string();

    // Spawn multiple concurrent tasks
    let mut handles = vec![];

    for i in 0..10 {
        let manager_clone = manager.clone();
        let image_path_clone = image_path.clone();

        let handle = task::spawn(async move {
            let service = manager_clone.create_twitter_service();
            service
                .post(&format!("Concurrent message {}", i), &image_path_clone)
                .await
        });

        handles.push(handle);
    }

    // Wait for all tasks to complete
    for handle in handles {
        assert!(handle.await.unwrap().is_ok());
    }

    // Verify all posts were recorded
    assert_eq!(manager.get_total_post_count(), 10);
    let posts = manager.get_posts_for_service("MockTwitter");
    assert_eq!(posts.len(), 10);
}

#[tokio::test]
async fn test_service_name_verification() {
    let manager = MockServiceManager::default();

    let twitter = manager.create_twitter_service();
    let mastodon = manager.create_mastodon_service();
    let bluesky = manager.create_bluesky_service();
    let kafka = manager.create_kafka_service();

    // Verify service names
    assert_eq!(twitter.name(), "MockTwitter");
    assert_eq!(mastodon.name(), "MockMastodon");
    assert_eq!(bluesky.name(), "MockBluesky");
    assert_eq!(kafka.name(), "MockKafka");
}

#[tokio::test]
async fn test_realistic_detection_workflow() {
    // Simulate a realistic detection workflow
    let manager = MockServiceManager::default();

    // Create services as individual variables with proper types
    let twitter_service = manager.create_twitter_service();
    let mastodon_service = manager.create_mastodon_service();
    let bluesky_service = manager.create_bluesky_service();
    let kafka_service = manager.create_kafka_service();

    let temp_file = create_test_image_file().unwrap();
    let image_path = temp_file.path().to_string_lossy().to_string();

    // Simulate detection scenario
    let detection_message = "🦭 RARE SAIMAA RINGED SEAL DETECTED! 📸 Location: Lake Saimaa, Finland. Confidence: 95%. This endangered species sighting has been automatically verified. #SaimaaRingedSeal #Wildlife #Conservation";

    // Post to Kafka first (for logging/analytics)
    kafka_service
        .post(detection_message, &image_path)
        .await
        .unwrap();

    // Post to social media services
    twitter_service
        .post(detection_message, &image_path)
        .await
        .unwrap();
    mastodon_service
        .post(detection_message, &image_path)
        .await
        .unwrap();
    bluesky_service
        .post(detection_message, &image_path)
        .await
        .unwrap();

    // Verify all posts were made
    assert_eq!(manager.get_total_post_count(), 4); // 3 social + 1 kafka

    // Verify Kafka post
    let kafka_posts = manager.get_posts_for_service("MockKafka");
    assert_eq!(kafka_posts.len(), 1);
    assert!(kafka_posts[0].message.contains("RARE SAIMAA RINGED SEAL"));

    // Verify social media posts
    for service_name in ["MockTwitter", "MockMastodon", "MockBluesky"] {
        let posts = manager.get_posts_for_service(service_name);
        assert_eq!(posts.len(), 1);
        assert!(posts[0].message.contains("🦭"));
        assert!(posts[0].message.contains("Confidence: 95%"));
    }
}
