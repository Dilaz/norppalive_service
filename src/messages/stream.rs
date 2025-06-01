use crate::error::NorppaliveError;
use actix::prelude::*;

/// Messages for StreamActor

#[derive(Message)]
#[rtype(result = "Result<(), crate::error::NorppaliveError>")]
pub struct StartStream {
    pub stream_url: String,
}

#[derive(Message)]
#[rtype(result = "Result<(), crate::error::NorppaliveError>")]
pub struct StopStream;

#[derive(Message)]
#[rtype(result = "()")]
pub struct FrameExtracted {
    pub frame_data: Vec<u8>,
    pub timestamp: i64,
    pub frame_index: u64,
}

/// Sent from the stream processing task to the StreamActor itself
#[derive(Message)]
#[rtype(result = "()")]
pub struct LatestFrameAvailable {
    pub frame_data: Vec<u8>, // RGB24 raw data
    pub width: u32,
    pub height: u32,
    pub timestamp: i64,
    pub frame_index: u64,
}

/// Sent from DetectionActor to StreamActor when it's ready for a new frame
#[derive(Message)]
#[rtype(result = "()")]
pub struct DetectorReady;

#[derive(Message)]
#[rtype(result = "()")]
pub struct InternalProcessingComplete {
    pub result: Result<(), NorppaliveError>,
}
