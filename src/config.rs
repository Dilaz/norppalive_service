use lazy_static::lazy_static;
use rand::rng;
use rand::seq::IndexedRandom;
use serde::Deserialize;

lazy_static! {
    pub static ref CONFIG: Config = {
        let config_path =
            std::env::var("CONFIG_PATH").unwrap_or_else(|_| "config.toml".to_string());

        let mut config: Config = match std::fs::read_to_string(&config_path) {
            Ok(config_content) => toml::from_str(&config_content)
                .unwrap_or_else(|_| panic!("Failed to parse config file: {}", config_path)),
            Err(_) => {
                // Provide a default configuration for tests or when config is missing
                Config::default()
            }
        };
        config.apply_env_overrides();
        config
    };
}

impl Config {
    /// Override credential fields from env vars when present. Lets the
    /// non-secret config live in a ConfigMap while secrets come from a
    /// (Sealed)Secret mounted via `envFrom`.
    fn apply_env_overrides(&mut self) {
        fn env(key: &str) -> Option<String> {
            std::env::var(key).ok().filter(|s| !s.is_empty())
        }
        if let Some(v) = env("MASTODON_HOST") { self.mastodon.host = v; }
        if let Some(v) = env("MASTODON_TOKEN") { self.mastodon.token = v; }
        if let Some(v) = env("BLUESKY_HOST") { self.bluesky.host = v; }
        if let Some(v) = env("BLUESKY_LOGIN") { self.bluesky.login = v; }
        if let Some(v) = env("BLUESKY_HANDLE") { self.bluesky.handle = v; }
        if let Some(v) = env("BLUESKY_PASSWORD") { self.bluesky.password = v; }
        if let Some(v) = env("TWITTER_TOKEN") { self.twitter.token = v; }
        if let Some(v) = env("TWITTER_TOKEN_SECRET") { self.twitter.token_secret = v; }
        if let Some(v) = env("TWITTER_CONSUMER_KEY") { self.twitter.consumer_key = v; }
        if let Some(v) = env("TWITTER_CONSUMER_SECRET") { self.twitter.consumer_secret = v; }
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            image_filename: "frames/frame.png".to_string(),
            stream: Stream {
                stream_url: "https://example.com/stream".to_string(),
                only_keyframes: true,
                max_frames: None,
                frame_processing_delay_ms: 100,
            },
            detection: Detection {
                minimum_detection_percentage: 75,
                minimum_detection_frames: 5,
                api_url: "http://localhost:8000/detect".to_string(),
                ignore_points: vec![],
                save_image_confidence: 65,
                minimum_post_confidence: 85,
            },
            output: Output {
                post_interval: 120,
                image_save_interval: 10,
                line_color: [255, 112, 32, 1],
                text_color: [0, 0, 0, 1],
                line_thickness: 5,
                output_file_folder: "saved_images".to_string(),
                replace_hashtags: false,
                messages: vec!["Test message".to_string()],
                services: vec![],
            },
            twitter: Twitter {
                token: String::new(),
                token_secret: String::new(),
                consumer_key: String::new(),
                consumer_secret: String::new(),
            },
            mastodon: Mastodon {
                host: String::new(),
                token: String::new(),
            },
            bluesky: Bluesky {
                host: "https://bsky.social".to_string(),
                login: String::new(),
                handle: String::new(),
                password: String::new(),
            },
            kafka: Kafka {
                broker: "localhost:19092".to_string(),
                topic: "detection".to_string(),
            },
        }
    }
}

#[derive(Deserialize, Debug)]
pub struct Config {
    pub image_filename: String,
    pub stream: Stream,
    pub detection: Detection,
    pub output: Output,
    pub twitter: Twitter,
    pub mastodon: Mastodon,
    pub bluesky: Bluesky,
    pub kafka: Kafka,
}

#[derive(Deserialize, Debug)]
pub struct Stream {
    pub stream_url: String,
    pub only_keyframes: bool,
    pub max_frames: Option<u64>, // Optional limit on number of frames to process
    pub frame_processing_delay_ms: u64, // Minimum interval between frame processing in milliseconds
}

#[derive(Deserialize, Debug)]
pub struct Detection {
    pub minimum_detection_percentage: u8,
    pub minimum_detection_frames: u32,
    pub api_url: String,
    pub ignore_points: Vec<Point>,
    pub save_image_confidence: u8,
    pub minimum_post_confidence: u8, // Minimum confidence required to post to social media
}

#[derive(Deserialize, Debug)]
pub struct Point {
    pub x: u32,
    pub y: u32,
}

#[derive(Deserialize, Debug, Clone)]
pub enum Service {
    Twitter,
    Mastodon,
    Bluesky,
    Kafka,
}
#[derive(Deserialize, Debug)]
pub struct Output {
    pub post_interval: i64,
    pub image_save_interval: i64,
    pub line_color: [u8; 4],
    pub text_color: [u8; 4],
    pub line_thickness: u32,
    pub output_file_folder: String,
    pub replace_hashtags: bool,
    pub messages: Vec<String>,
    // pub kvstore_url: String,
    // pub kvstore_token: String,
    pub services: Vec<Service>,
}

impl Output {
    pub fn get_random_message(&self) -> Option<&String> {
        if self.messages.is_empty() {
            return None;
        }
        self.messages.choose(&mut rng())
    }
}

#[derive(Deserialize, Debug)]
pub struct Twitter {
    pub token: String,
    pub token_secret: String,
    pub consumer_key: String,
    pub consumer_secret: String,
}

#[derive(Deserialize, Debug)]
pub struct Mastodon {
    pub host: String,
    pub token: String,
}

#[derive(Deserialize, Debug)]
pub struct Bluesky {
    pub host: String,
    pub login: String,
    pub handle: String,
    pub password: String,
}

#[derive(Deserialize, Debug)]
pub struct Kafka {
    pub broker: String,
    pub topic: String,
}
