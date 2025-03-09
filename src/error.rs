use miette::Diagnostic;
use thiserror::Error;

#[derive(Debug, Diagnostic, Error)]
pub enum NorppaliveError {
    // ServiceError(String),
    #[error(transparent)]
    FFmpegError(#[from] ffmpeg_next::Error),

    #[error(transparent)]
    IOError(#[from] std::io::Error),

    #[error(transparent)]
    ImageError(#[from] image::ImageError),

    #[error(transparent)]
    FontError(#[from] ab_glyph::InvalidFont),

    #[error(transparent)]
    JsonError(#[from] serde_json::Error),

    #[error(transparent)]
    TwitterEndpointError(#[from] twitter_api_v1::endpoints::EndpointError),

    #[error(transparent)]
    Twitterv2Error(#[from] twitter_v2::Error),

    #[error(transparent)]
    AtriumError(#[from] atrium_api::error::Error),

    #[error(transparent)]
    AtriumCreateRecordError(
        #[from] atrium_xrpc::error::Error<atrium_api::com::atproto::repo::create_record::Error>,
    ),

    #[error(transparent)]
    AtriumCreateSessionError(
        #[from] atrium_xrpc::error::Error<atrium_api::com::atproto::server::create_session::Error>,
    ),

    #[error(transparent)]
    AtriumUploadBlobError(
        #[from] atrium_xrpc::error::Error<atrium_api::com::atproto::repo::upload_blob::Error>,
    ),

    #[error(transparent)]
    MegalodonError(#[from] megalodon::error::Error),

    #[error(transparent)]
    KafkaError(#[from] rdkafka::error::KafkaError),

    #[error(transparent)]
    ReqwestError(#[from] reqwest::Error),

    #[error("Some other error: {0}")]
    Other(String),
}
