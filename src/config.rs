use serde::Deserialize;

#[derive(Deserialize, Debug)]
pub struct Config {
    pub image_filename: String,
    pub stream: Stream,
    pub detection: Detection,
    pub output: Output,
    pub twitter: Twitter,
    pub mastodon: Mastodon,
    pub bluesky: Bluesky,
}

#[derive(Deserialize, Debug)]
pub struct Stream {
    pub stream_url: String,
    pub only_keyframes: bool,
}

#[derive(Deserialize, Debug)]
pub struct Detection {
    pub minimum_detection_percentage: u8,
    pub minimum_detection_frames: u8,
    pub api_url: String,
    pub ignore_points: Vec<Point>,
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
    pub kvstore_url: String,
    pub kvstore_token: String,
    pub services: Vec<Service>,
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