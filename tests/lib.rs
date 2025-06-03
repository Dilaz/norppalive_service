// Common test utilities and re-exports for integration tests

// Common test setup
pub fn init_test_tracing() {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    let _ = tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new("debug"))
        .with(tracing_subscriber::fmt::layer().with_test_writer())
        .try_init();
}

// Test helper functions
pub fn create_test_frame() -> Vec<u8> {
    vec![0; 1920 * 1080 * 3] // Mock RGB frame data
}

pub fn create_small_test_frame() -> Vec<u8> {
    vec![0; 640 * 480 * 3] // Smaller mock RGB frame data
}
