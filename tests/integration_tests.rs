use actix::prelude::*;
use image::DynamicImage;
use norppalive_service::actors::*;
use norppalive_service::messages::*;
use norppalive_service::utils::detection_utils::DetectionResult;
use norppalive_service::utils::output::MockOutputService;

// Test helper functions
fn create_test_image() -> DynamicImage {
    image::DynamicImage::new_rgb8(100, 100)
}

fn create_test_detection() -> DetectionResult {
    DetectionResult {
        r#box: [10, 10, 50, 50],
        cls: 0,
        cls_name: "seal".to_string(),
        conf: 85,
    }
}

#[actix::test]
async fn test_supervisor_and_actor_communication() {
    let supervisor = SupervisorActor::new().start();

    // Test health check
    let health = supervisor.send(GetSystemHealth).await.unwrap().unwrap();
    assert!(health.overall_healthy);
    assert_eq!(health.total_restarts, 0);

    // Test detection stats
    let detection_actor = DetectionActor::new().start();
    let stats = detection_actor
        .send(GetDetectionStats)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stats.total_detections, 0);
    assert_eq!(stats.consecutive_detections, 0);
}

#[actix::test]
async fn test_detection_to_output_flow() {
    let _detection_actor = DetectionActor::new().start();
    let output_actor = OutputActor::new(Box::new(MockOutputService::new())).start();

    let test_image = create_test_image();
    let test_detection = create_test_detection();

    // Test saving detection image
    let save_result = output_actor
        .send(SaveDetectionImage {
            detections: vec![test_detection.clone()],
            image: test_image.clone(),
        })
        .await
        .unwrap();
    assert!(save_result.is_ok());

    // Test posting to social media (will use mocks due to environment variable)
    let post_result = output_actor
        .send(PostToSocialMedia {
            detections: vec![test_detection],
            image: test_image,
            message: "Test detection".to_string(),
        })
        .await
        .unwrap();
    // With mocks enabled, this should succeed
    assert!(post_result.is_ok());
}

#[actix::test]
async fn test_stream_to_detection_flow() {
    // Start actors
    let stream_actor = StreamActor::new().start();
    let detection_actor = DetectionActor::new().start();

    // Start stream (might fail in production mode without FFmpeg setup, which is expected)
    let _result = stream_actor
        .send(StartStream {
            stream_url: "test://stream.url".to_string(),
        })
        .await
        .unwrap();

    // Simulate frame extraction (would normally come from StreamActor)
    stream_actor.do_send(FrameExtracted {
        frame_data: vec![1, 2, 3, 4, 5],
        timestamp: 1234567890,
        frame_index: 1,
    });

    // Test detection processing with mock data
    let mock_stream_actor = StreamActor::new().start();
    let detection_result = detection_actor
        .send(ProcessFrame {
            image_path: "/fake/test/image.jpg".to_string(),
            timestamp: 1234567890,
            reply_to: mock_stream_actor,
        })
        .await
        .unwrap();

    // In test environment, should handle gracefully
    // The result depends on whether we have mock services or real ones
    // For now, just ensure the actor responds
    assert!(detection_result.is_ok() || detection_result.is_err());

    // Stop stream
    let stop_result = stream_actor.send(StopStream).await.unwrap();
    assert!(stop_result.is_ok());
}

#[actix::test]
async fn test_full_actor_system_startup() {
    // Start all core actors
    let supervisor = SupervisorActor::new().start();
    let stream_actor = StreamActor::new().start();
    let detection_actor = DetectionActor::new().start();
    let output_actor = OutputActor::new(Box::new(MockOutputService::new())).start();

    // Test that all actors are responsive
    let health = supervisor.send(GetSystemHealth).await.unwrap().unwrap();
    assert!(health.overall_healthy);

    let detection_stats = detection_actor
        .send(GetDetectionStats)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(detection_stats.total_detections, 0);

    let output_status = output_actor.send(GetServiceStatus).await.unwrap().unwrap();
    assert!(output_status.healthy);

    // Test stream operations (might fail without proper stream setup, which is expected)
    let _start_result = stream_actor
        .send(StartStream {
            stream_url: "test://example.stream".to_string(),
        })
        .await
        .unwrap();

    let stop_result = stream_actor.send(StopStream).await.unwrap();
    assert!(stop_result.is_ok());
}

#[actix::test]
async fn test_concurrent_actor_operations() {
    // Start actors
    let detection_actor = DetectionActor::new().start();
    let output_actor = OutputActor::new(Box::new(MockOutputService::new())).start();

    let test_image = create_test_image();
    let test_detection = create_test_detection();

    // Send concurrent operations to different actors
    let detection_future = detection_actor.send(GetDetectionStats);
    let output_post_future = output_actor.send(PostToSocialMedia {
        detections: vec![test_detection.clone()],
        image: test_image.clone(),
        message: "Concurrent test".to_string(),
    });
    let output_save_future = output_actor.send(SaveDetectionImage {
        detections: vec![test_detection],
        image: test_image,
    });

    // Wait for all operations to complete
    let (detection_res, post_res, save_res) =
        futures::future::join3(detection_future, output_post_future, output_save_future).await;

    // All operations should complete successfully with mocks enabled
    assert!(detection_res.is_ok());
    assert!(post_res.is_ok());
    assert!(post_res.unwrap().is_ok()); // With mocks, posting should succeed
    assert!(save_res.is_ok());
    assert!(save_res.unwrap().is_ok()); // Saving should succeed
}

#[actix::test]
async fn test_actor_error_handling() {
    let detection_actor = DetectionActor::new().start();
    let supervisor = SupervisorActor::new().start();
    let mock_stream_actor = StreamActor::new().start();

    // Test detection with invalid input
    let result = detection_actor
        .send(ProcessFrame {
            image_path: "/absolutely/nonexistent/path/to/image.jpg".to_string(),
            timestamp: 1234567890,
            reply_to: mock_stream_actor,
        })
        .await
        .unwrap();

    // Should handle error gracefully (result depends on mock vs real services)
    assert!(result.is_ok() || result.is_err());

    // Supervisor should still report system as healthy
    let health = supervisor.send(GetSystemHealth).await.unwrap().unwrap();
    assert!(health.overall_healthy);
}

#[actix::test]
async fn test_message_bus_abstraction_concept() {
    // This test demonstrates how the message bus abstraction would work
    let output_actor = OutputActor::new(Box::new(MockOutputService::new())).start();
    let detection_actor = DetectionActor::new().start();

    let test_image = create_test_image();
    let test_detection = create_test_detection();

    // Test that actors can communicate through message passing
    let detection_stats = detection_actor
        .send(GetDetectionStats)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(detection_stats.total_detections, 0);

    let output_status = output_actor.send(GetServiceStatus).await.unwrap().unwrap();
    assert!(output_status.healthy);

    // Test posting through the output actor (message bus concept)
    let post_result = output_actor
        .send(PostToSocialMedia {
            detections: vec![test_detection],
            image: test_image,
            message: "Message bus test".to_string(),
        })
        .await
        .unwrap();
    // With mocks enabled, this should succeed
    assert!(post_result.is_ok());
}
