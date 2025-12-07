use miette::Diagnostic;
use thiserror::Error;

#[derive(Debug, Diagnostic, Error)]
pub enum NorppaliveError {
    // Box larger error variants to reduce overall enum size
    #[error(transparent)]
    #[diagnostic(
        code(norppalive::ffmpeg_error),
        help("An error occurred during video processing with FFmpeg.")
    )]
    FFmpegError(Box<ffmpeg_next::Error>),

    #[error(transparent)]
    #[diagnostic(code(norppalive::io_error), help("An I/O error occurred."))]
    IOError(Box<std::io::Error>),

    #[error(transparent)]
    #[diagnostic(
        code(norppalive::image_error),
        help("An error occurred during image processing.")
    )]
    ImageError(Box<image::ImageError>),

    #[error(transparent)]
    #[diagnostic(
        code(norppalive::font_error),
        help("An error occurred with font loading or rendering.")
    )]
    FontError(#[from] ab_glyph::InvalidFont),

    #[error(transparent)]
    #[diagnostic(
        code(norppalive::json_error),
        help("An error occurred during JSON serialization or deserialization.")
    )]
    JsonError(Box<serde_json::Error>),

    #[error(transparent)]
    #[diagnostic(
        code(norppalive::twitter_endpoint_error),
        help("An error occurred with a Twitter API v1 endpoint.")
    )]
    TwitterEndpointError(Box<twitter_api_v1::endpoints::EndpointError>),

    #[error(transparent)]
    #[diagnostic(
        code(norppalive::twitter_v2_error),
        help("An error occurred with the Twitter API v2.")
    )]
    Twitterv2Error(Box<twitter_v2::Error>),

    #[error(transparent)]
    #[diagnostic(
        code(norppalive::atrium_error),
        help("An error occurred with the Atrium API.")
    )]
    AtriumError(Box<atrium_api::error::Error>),

    #[error(transparent)]
    #[diagnostic(
        code(norppalive::atrium_create_record_error),
        help("Failed to create a record via Atrium API.")
    )]
    AtriumCreateRecordError(
        Box<atrium_xrpc::error::Error<atrium_api::com::atproto::repo::create_record::Error>>,
    ),

    #[error(transparent)]
    #[diagnostic(
        code(norppalive::atrium_create_session_error),
        help("Failed to create a session via Atrium API.")
    )]
    AtriumCreateSessionError(
        Box<atrium_xrpc::error::Error<atrium_api::com::atproto::server::create_session::Error>>,
    ),

    #[error(transparent)]
    #[diagnostic(
        code(norppalive::atrium_upload_blob_error),
        help("Failed to upload a blob via Atrium API.")
    )]
    AtriumUploadBlobError(
        Box<atrium_xrpc::error::Error<atrium_api::com::atproto::repo::upload_blob::Error>>,
    ),

    #[error(transparent)]
    #[diagnostic(
        code(norppalive::megalodon_error),
        help("An error occurred with the Megalodon library (Mastodon API).")
    )]
    MegalodonError(Box<megalodon::error::Error>),

    #[error(transparent)]
    #[diagnostic(code(norppalive::kafka_error), help("An error occurred with Kafka."))]
    KafkaError(Box<rdkafka::error::KafkaError>),

    #[error(transparent)]
    #[diagnostic(
        code(norppalive::reqwest_error),
        help("An HTTP request error occurred using Reqwest.")
    )]
    ReqwestError(Box<reqwest::Error>),

    #[error("Failed to process stream URL: {0}")]
    #[diagnostic(
        code(norppalive::stream_url_error),
        help("An error occurred while trying to obtain or process a stream URL, often involving yt-dlp.")
    )]
    StreamUrlError(String),

    #[error("Some other error: {0}")]
    #[diagnostic(code(norppalive::other_error), help("An unspecified error occurred."))]
    Other(String),
}

// Manual From implementations for boxed error types
impl From<ffmpeg_next::Error> for NorppaliveError {
    fn from(err: ffmpeg_next::Error) -> Self {
        NorppaliveError::FFmpegError(Box::new(err))
    }
}

impl From<std::io::Error> for NorppaliveError {
    fn from(err: std::io::Error) -> Self {
        NorppaliveError::IOError(Box::new(err))
    }
}

impl From<image::ImageError> for NorppaliveError {
    fn from(err: image::ImageError) -> Self {
        NorppaliveError::ImageError(Box::new(err))
    }
}

impl From<serde_json::Error> for NorppaliveError {
    fn from(err: serde_json::Error) -> Self {
        NorppaliveError::JsonError(Box::new(err))
    }
}

impl From<twitter_api_v1::endpoints::EndpointError> for NorppaliveError {
    fn from(err: twitter_api_v1::endpoints::EndpointError) -> Self {
        NorppaliveError::TwitterEndpointError(Box::new(err))
    }
}

impl From<twitter_v2::Error> for NorppaliveError {
    fn from(err: twitter_v2::Error) -> Self {
        NorppaliveError::Twitterv2Error(Box::new(err))
    }
}

impl From<atrium_api::error::Error> for NorppaliveError {
    fn from(err: atrium_api::error::Error) -> Self {
        NorppaliveError::AtriumError(Box::new(err))
    }
}

impl From<atrium_xrpc::error::Error<atrium_api::com::atproto::repo::create_record::Error>>
    for NorppaliveError
{
    fn from(
        err: atrium_xrpc::error::Error<atrium_api::com::atproto::repo::create_record::Error>,
    ) -> Self {
        NorppaliveError::AtriumCreateRecordError(Box::new(err))
    }
}

impl From<atrium_xrpc::error::Error<atrium_api::com::atproto::server::create_session::Error>>
    for NorppaliveError
{
    fn from(
        err: atrium_xrpc::error::Error<atrium_api::com::atproto::server::create_session::Error>,
    ) -> Self {
        NorppaliveError::AtriumCreateSessionError(Box::new(err))
    }
}

impl From<atrium_xrpc::error::Error<atrium_api::com::atproto::repo::upload_blob::Error>>
    for NorppaliveError
{
    fn from(
        err: atrium_xrpc::error::Error<atrium_api::com::atproto::repo::upload_blob::Error>,
    ) -> Self {
        NorppaliveError::AtriumUploadBlobError(Box::new(err))
    }
}

impl From<megalodon::error::Error> for NorppaliveError {
    fn from(err: megalodon::error::Error) -> Self {
        NorppaliveError::MegalodonError(Box::new(err))
    }
}

impl From<rdkafka::error::KafkaError> for NorppaliveError {
    fn from(err: rdkafka::error::KafkaError) -> Self {
        NorppaliveError::KafkaError(Box::new(err))
    }
}

impl From<reqwest::Error> for NorppaliveError {
    fn from(err: reqwest::Error) -> Self {
        NorppaliveError::ReqwestError(Box::new(err))
    }
}

impl NorppaliveError {
    /// Creates a simplified, cloneable representation of the error for reporting purposes.
    /// Converts non-Clone error types to `NorppaliveError::Other` with their string representation.
    pub fn clone_for_error_reporting(&self) -> Self {
        match self {
            // String-based variants can be cloned directly
            NorppaliveError::StreamUrlError(s) => NorppaliveError::StreamUrlError(s.clone()),
            NorppaliveError::Other(s) => NorppaliveError::Other(s.clone()),
            // All other variants are converted to Other with their Display representation
            _ => NorppaliveError::Other(self.to_string()),
        }
    }
}
