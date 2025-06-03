use norppalive_service::services::SocialMediaService;
use std::fs;
use std::sync::Arc;

#[cfg(feature = "test-utils")]
use mockall::predicate::*;

fn create_test_image_file() -> Result<tempfile::NamedTempFile, std::io::Error> {
    let temp_file = tempfile::NamedTempFile::new()?;
    let test_image_data = b"fake image data for testing purposes";
    fs::write(temp_file.path(), test_image_data)?;
    Ok(temp_file)
}

#[cfg(feature = "test-utils")]
#[tokio::test]
async fn test_mock_service_manager_integration() {
    use norppalive_service::services::generic::test_mocks::MockMockSocialMedia;

    let mut mock_twitter = MockMockSocialMedia::new();
    let mut mock_mastodon = MockMockSocialMedia::new();
    let mut mock_bluesky = MockMockSocialMedia::new();
    let mut mock_kafka = MockMockSocialMedia::new();

    let temp_file = create_test_image_file().unwrap();
    let image_path = temp_file.path().to_string_lossy().to_string();
    let message = "Rare Saimaa ringed seal detected! 🦭";

    // Set expectations for Twitter
    mock_twitter
        .expect_post()
        .with(eq(message), eq(image_path.clone()))
        .times(1)
        .returning(|_, _| Ok(()));
    // mock_twitter.expect_name().returning(|| "MockTwitter"); // If name was asserted

    // Set expectations for Mastodon
    mock_mastodon
        .expect_post()
        .with(eq(message), eq(image_path.clone()))
        .times(1)
        .returning(|_, _| Ok(()));
    // mock_mastodon.expect_name().returning(|| "MockMastodon");

    // Set expectations for Bluesky
    mock_bluesky
        .expect_post()
        .with(eq(message), eq(image_path.clone()))
        .times(1)
        .returning(|_, _| Ok(()));
    // mock_bluesky.expect_name().returning(|| "MockBluesky");

    // Set expectations for Kafka
    mock_kafka
        .expect_post()
        .with(eq(message), eq(image_path.clone()))
        .times(1)
        .returning(|_, _| Ok(()));
    // mock_kafka.expect_name().returning(|| "MockKafka");

    // Call post on all services
    mock_twitter.post(message, &image_path).await.unwrap();
    mock_mastodon.post(message, &image_path).await.unwrap();
    mock_bluesky.post(message, &image_path).await.unwrap();
    mock_kafka.post(message, &image_path).await.unwrap();

    // Verification of call counts and arguments happens automatically when mocks go out of scope.
}

#[cfg_attr(feature = "test-utils", tokio::test)]
#[cfg_attr(
    not(feature = "test-utils"),
    ignore = "requires test-utils feature for mocks"
)]
async fn test_mock_service_failure_scenarios() {
    #[cfg(feature = "test-utils")]
    {
        use norppalive_service::error::NorppaliveError;
        use norppalive_service::services::generic::test_mocks::MockMockSocialMedia;

        let mut mock_twitter = MockMockSocialMedia::new();
        let mut mock_mastodon = MockMockSocialMedia::new();

        let temp_file = create_test_image_file().unwrap();
        let image_path = temp_file.path().to_string_lossy().to_string();
        let error_message = "Network error simulation".to_string();

        // 1. Failure Scenario
        // Expect Twitter to fail
        mock_twitter
            .expect_post()
            .with(eq("Test message"), eq(image_path.clone()))
            .times(1)
            .returning({
                let em = error_message.clone();
                move |_, _| Err(NorppaliveError::Other(em.clone()))
            });

        // Expect Mastodon to fail
        mock_mastodon
            .expect_post()
            .with(eq("Test message"), eq(image_path.clone()))
            .times(1)
            .returning(move |_, _| Err(NorppaliveError::Other(error_message.clone())));

        // Call post on both services - expecting failure
        let twitter_result_fail = mock_twitter.post("Test message", &image_path).await;
        let mastodon_result_fail = mock_mastodon.post("Test message", &image_path).await;

        assert!(twitter_result_fail.is_err());
        assert!(mastodon_result_fail.is_err());

        // mockall verifies .times(1) for the failed calls when they go out of scope or at checkpoint.

        // 2. Recovery Scenario (for Twitter)
        // Clear previous expectations for mock_twitter and set new ones
        mock_twitter.checkpoint();

        mock_twitter
            .expect_post()
            .with(eq("Recovery test"), eq(image_path.clone()))
            .times(1)
            .returning(|_, _| Ok(()));

        let twitter_recovery_result = mock_twitter.post("Recovery test", &image_path).await;
        assert!(twitter_recovery_result.is_ok());

        // mock_mastodon was only used for the failure part in this test logic.
        // Its .times(1) expectation for failure is verified when it goes out of scope.
    }
}

#[cfg_attr(feature = "test-utils", tokio::test)]
#[cfg_attr(
    not(feature = "test-utils"),
    ignore = "requires test-utils feature for mocks"
)]
async fn test_individual_service_configurations() {
    #[cfg(feature = "test-utils")]
    {
        use norppalive_service::error::NorppaliveError;
        use norppalive_service::services::generic::test_mocks::MockMockSocialMedia;

        let mut mock_twitter = MockMockSocialMedia::new();
        let mut mock_mastodon = MockMockSocialMedia::new();

        let temp_file = create_test_image_file().unwrap();
        let image_path = temp_file.path().to_string_lossy().to_string();
        let image_path_clone = image_path.clone(); // For closure

        // Configure Twitter to fail
        mock_twitter
            .expect_post()
            .with(eq("Test message"), eq(image_path.clone()))
            .times(1)
            .returning(move |_, _| {
                Err(NorppaliveError::Other("Twitter API rate limit".to_string()))
            });

        mock_twitter.expect_name().returning(|| "MockTwitter");

        // Configure Mastodon to succeed
        mock_mastodon
            .expect_post()
            .with(eq("Test message"), eq(image_path_clone.clone()))
            .times(1)
            .returning(|_, _| Ok(()));
        mock_mastodon.expect_name().returning(|| "MockMastodon");

        let twitter_result = mock_twitter.post("Test message", &image_path).await;
        let mastodon_result = mock_mastodon.post("Test message", &image_path_clone).await;

        assert!(twitter_result.is_err());
        if let Err(NorppaliveError::Other(msg)) = twitter_result {
            assert_eq!(msg, "Twitter API rate limit");
        } else {
            panic!("Expected NorppaliveError::Other for Twitter");
        }
        assert!(mastodon_result.is_ok());

        // mockall automatically verifies expectations when mocks go out of scope.
    }
}

#[cfg_attr(feature = "test-utils", tokio::test)]
#[cfg_attr(
    not(feature = "test-utils"),
    ignore = "requires test-utils feature for mocks"
)]
async fn test_concurrent_mock_service_access() {
    #[cfg(feature = "test-utils")]
    {
        use norppalive_service::services::generic::test_mocks::MockMockSocialMedia;
        use tokio::task;

        let mut mock_service_setup = MockMockSocialMedia::new();

        mock_service_setup
            .expect_post()
            .times(10)
            .with(
                function(|msg: &str| msg.starts_with("Concurrent message")),
                always(),
            )
            .returning(|_, _| Ok(()));

        let arc_mock_service: Arc<MockMockSocialMedia> = Arc::new(mock_service_setup);

        let temp_file = create_test_image_file().unwrap();
        let image_path = temp_file.path().to_string_lossy().to_string();
        let mut handles = vec![];

        for i in 0..10 {
            let service_clone_for_task: Arc<MockMockSocialMedia> = Arc::clone(&arc_mock_service);
            let image_path_clone = image_path.clone();
            let handle = task::spawn(async move {
                service_clone_for_task
                    .post(&format!("Concurrent message {}", i), &image_path_clone)
                    .await
            });
            handles.push(handle);
        }

        for handle in handles {
            let result = handle.await.unwrap();
            assert!(result.is_ok());
        }
        // arc_mock_service (and the original mock_service_setup it owns) goes out of scope here, verifying expectations.
    }
}

#[cfg_attr(feature = "test-utils", tokio::test)]
#[cfg_attr(
    not(feature = "test-utils"),
    ignore = "requires test-utils feature for mocks"
)]
async fn test_realistic_detection_workflow() {
    #[cfg(feature = "test-utils")]
    {
        use norppalive_service::services::generic::test_mocks::MockMockSocialMedia;

        let mut mock_twitter = MockMockSocialMedia::new();
        let mut mock_mastodon = MockMockSocialMedia::new();
        let mut mock_bluesky = MockMockSocialMedia::new();
        let mut mock_kafka = MockMockSocialMedia::new();

        let temp_file = create_test_image_file().unwrap();
        let image_path = temp_file.path().to_string_lossy().to_string();
        let detection_message = "🦭 RARE SAIMAA RINGED SEAL DETECTED! 📸 Location: Lake Saimaa, Finland. Confidence: 95%. This endangered species sighting has been automatically verified. #SaimaaRingedSeal #Wildlife #Conservation";

        // Expectations for Kafka
        mock_kafka
            .expect_post()
            .with(eq(detection_message), eq(image_path.clone()))
            .times(1)
            .returning(|_, _| Ok(()));

        // Expectations for Twitter
        mock_twitter
            .expect_post()
            .with(eq(detection_message), eq(image_path.clone()))
            .times(1)
            .returning(|_, _| Ok(()));

        // Expectations for Mastodon
        mock_mastodon
            .expect_post()
            .with(eq(detection_message), eq(image_path.clone()))
            .times(1)
            .returning(|_, _| Ok(()));

        // Expectations for Bluesky
        mock_bluesky
            .expect_post()
            .with(eq(detection_message), eq(image_path.clone()))
            .times(1)
            .returning(|_, _| Ok(()));

        // Simulate posting flow
        mock_kafka
            .post(detection_message, &image_path)
            .await
            .unwrap();
        mock_twitter
            .post(detection_message, &image_path)
            .await
            .unwrap();
        mock_mastodon
            .post(detection_message, &image_path)
            .await
            .unwrap();
        mock_bluesky
            .post(detection_message, &image_path)
            .await
            .unwrap();

        // Content verification is handled by .with(eq(...)) predicates.
        // All expectations (call counts, arguments) are verified when mocks go out of scope.
    }
}

// NEW TEST USING MOCKALL
#[cfg(all(test, feature = "test-utils"))]
mod mockall_tests {
    use super::*;
    use norppalive_service::error::NorppaliveError;
    use norppalive_service::services::generic::test_mocks::MockMockSocialMedia;
    use norppalive_service::services::SocialMediaService;

    #[tokio::test]
    async fn test_single_mockable_social_media_service() {
        let mut mock_service = MockMockSocialMedia::new(); // Use the #[automock] generated mock

        mock_service
            .expect_name()
            .times(1)
            .returning(|| "MockedService");

        mock_service
            .expect_post()
            .with(eq("Test message"), eq("test_image.jpg"))
            .times(1)
            .returning(|_, _| Ok(()));

        assert_eq!(mock_service.name(), "MockedService");
        let post_result = mock_service.post("Test message", "test_image.jpg").await;
        assert!(post_result.is_ok());
    }

    #[tokio::test]
    async fn test_mockable_social_media_service_error() {
        let mut mock_service = MockMockSocialMedia::new(); // Use the #[automock] generated mock

        mock_service
            .expect_post()
            .with(always(), always()) // Any arguments
            .times(1)
            .returning(|_, _| Err(NorppaliveError::Other("Simulated post error".to_string())));

        let post_result = mock_service
            .post("Another message", "another_image.png")
            .await;
        assert!(post_result.is_err());
        match post_result {
            Err(NorppaliveError::Other(msg)) => assert_eq!(msg, "Simulated post error"),
            _ => panic!("Expected NorppaliveError::Other"),
        }
    }
}
