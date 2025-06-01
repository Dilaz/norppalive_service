use miette::Diagnostic;
use thiserror::Error;

#[derive(Debug, Diagnostic, Error)]
pub enum NorppaliveError {
    // ServiceError(String),
    #[error(transparent)]
    #[diagnostic(
        code(norppalive::ffmpeg_error),
        help("An error occurred during video processing with FFmpeg.")
    )]
    FFmpegError(#[from] ffmpeg_next::Error),

    #[error(transparent)]
    #[diagnostic(code(norppalive::io_error), help("An I/O error occurred."))]
    IOError(#[from] std::io::Error),

    #[error(transparent)]
    #[diagnostic(
        code(norppalive::image_error),
        help("An error occurred during image processing.")
    )]
    ImageError(#[from] image::ImageError),

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
    JsonError(#[from] serde_json::Error),

    #[error(transparent)]
    #[diagnostic(
        code(norppalive::twitter_endpoint_error),
        help("An error occurred with a Twitter API v1 endpoint.")
    )]
    TwitterEndpointError(#[from] twitter_api_v1::endpoints::EndpointError),

    #[error(transparent)]
    #[diagnostic(
        code(norppalive::twitter_v2_error),
        help("An error occurred with the Twitter API v2.")
    )]
    Twitterv2Error(#[from] twitter_v2::Error),

    #[error(transparent)]
    #[diagnostic(
        code(norppalive::atrium_error),
        help("An error occurred with the Atrium API.")
    )]
    AtriumError(#[from] atrium_api::error::Error),

    #[error(transparent)]
    #[diagnostic(
        code(norppalive::atrium_create_record_error),
        help("Failed to create a record via Atrium API.")
    )]
    AtriumCreateRecordError(
        #[from] atrium_xrpc::error::Error<atrium_api::com::atproto::repo::create_record::Error>,
    ),

    #[error(transparent)]
    #[diagnostic(
        code(norppalive::atrium_create_session_error),
        help("Failed to create a session via Atrium API.")
    )]
    AtriumCreateSessionError(
        #[from] atrium_xrpc::error::Error<atrium_api::com::atproto::server::create_session::Error>,
    ),

    #[error(transparent)]
    #[diagnostic(
        code(norppalive::atrium_upload_blob_error),
        help("Failed to upload a blob via Atrium API.")
    )]
    AtriumUploadBlobError(
        #[from] atrium_xrpc::error::Error<atrium_api::com::atproto::repo::upload_blob::Error>,
    ),

    // Box the largest error variant to reduce overall enum size
    #[error(transparent)]
    #[diagnostic(
        code(norppalive::megalodon_error),
        help("An error occurred with the Megalodon library (Mastodon API).")
    )]
    MegalodonError(Box<megalodon::error::Error>),

    #[error(transparent)]
    #[diagnostic(code(norppalive::kafka_error), help("An error occurred with Kafka."))]
    KafkaError(#[from] rdkafka::error::KafkaError),

    #[error(transparent)]
    #[diagnostic(
        code(norppalive::reqwest_error),
        help("An HTTP request error occurred using Reqwest.")
    )]
    ReqwestError(#[from] reqwest::Error),

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

// Manual From implementation for the boxed MegalodonError
impl From<megalodon::error::Error> for NorppaliveError {
    fn from(err: megalodon::error::Error) -> Self {
        NorppaliveError::MegalodonError(Box::new(err))
    }
}

impl NorppaliveError {
    /// Creates a simplified, cloneable representation of the error for reporting purposes.
    /// Many underlying error types are not `Clone`, so this converts them to a String representation
    /// wrapped in `NorppaliveError::Other` or clones the variant if it's already simple (e.g., String-based).
    pub fn clone_for_error_reporting(&self) -> Self {
        match self {
            NorppaliveError::FFmpegError(e) => {
                NorppaliveError::Other(format!("FFmpegError: {}", e))
            }
            NorppaliveError::IOError(e) => NorppaliveError::Other(format!("IOError: {}", e)),
            NorppaliveError::ImageError(e) => NorppaliveError::Other(format!("ImageError: {}", e)),
            NorppaliveError::FontError(e) => NorppaliveError::Other(format!("FontError: {}", e)),
            NorppaliveError::JsonError(e) => NorppaliveError::Other(format!("JsonError: {}", e)),
            NorppaliveError::TwitterEndpointError(e) => {
                NorppaliveError::Other(format!("TwitterEndpointError: {}", e))
            }
            NorppaliveError::Twitterv2Error(e) => {
                NorppaliveError::Other(format!("Twitterv2Error: {}", e))
            }
            NorppaliveError::AtriumError(e) => {
                NorppaliveError::Other(format!("AtriumError: {}", e))
            }
            NorppaliveError::AtriumCreateRecordError(e) => {
                NorppaliveError::Other(format!("AtriumCreateRecordError: {}", e))
            }
            NorppaliveError::AtriumCreateSessionError(e) => {
                NorppaliveError::Other(format!("AtriumCreateSessionError: {}", e))
            }
            NorppaliveError::AtriumUploadBlobError(e) => {
                NorppaliveError::Other(format!("AtriumUploadBlobError: {}", e))
            }
            NorppaliveError::MegalodonError(e) => {
                NorppaliveError::Other(format!("MegalodonError: {}", e))
            }
            NorppaliveError::KafkaError(e) => NorppaliveError::Other(format!("KafkaError: {}", e)),
            NorppaliveError::ReqwestError(e) => {
                NorppaliveError::Other(format!("ReqwestError: {}", e))
            }
            NorppaliveError::StreamUrlError(s) => NorppaliveError::StreamUrlError(s.clone()),
            NorppaliveError::Other(s) => NorppaliveError::Other(s.clone()),
        }
    }
}
