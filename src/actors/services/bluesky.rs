use actix::prelude::*;
use chrono::Utc;
use tracing::{error, info};

use crate::error::NorppaliveError;
use crate::messages::{GetServiceStatus, ServicePost, ServicePostResult, ServiceStatus};
use crate::services::{BlueskyService, SocialMediaService};

/// BlueskyActor handles posting to Bluesky
pub struct BlueskyActor {
    service_status: ServiceStatus,
}

impl Default for BlueskyActor {
    fn default() -> Self {
        Self {
            service_status: ServiceStatus {
                name: "Bluesky".to_string(),
                healthy: true,
                last_post_time: None,
                error_count: 0,
                rate_limited: false,
            },
        }
    }
}

impl Actor for BlueskyActor {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        info!("BlueskyActor started");
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!("BlueskyActor stopped");
    }
}

impl BlueskyActor {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Handler<ServicePost> for BlueskyActor {
    type Result = ResponseActFuture<Self, Result<ServicePostResult, NorppaliveError>>;

    fn handle(&mut self, msg: ServicePost, _ctx: &mut Self::Context) -> Self::Result {
        info!("Received Bluesky post request: {}", msg.message);

        let message = msg.message.clone();
        let image_path = msg.image_path.clone();

        Box::pin(
            async move {
                // Use real Bluesky service
                let bluesky_service = BlueskyService::default();
                match bluesky_service.post(&message, &image_path).await {
                    Ok(_) => {
                        info!("Successfully posted to Bluesky");
                        Ok(ServicePostResult {
                            success: true,
                            service_name: "Bluesky".to_string(),
                            error_message: None,
                            posted_at: Utc::now().timestamp(),
                        })
                    }
                    Err(err) => {
                        error!("Failed to post to Bluesky: {}", err);
                        Ok(ServicePostResult {
                            success: false,
                            service_name: "Bluesky".to_string(),
                            error_message: Some(err.to_string()),
                            posted_at: Utc::now().timestamp(),
                        })
                    }
                }
            }
            .into_actor(self)
            .map(|result, actor, _ctx| {
                if let Ok(ref post_result) = result {
                    if post_result.success {
                        actor.service_status.last_post_time = Some(Utc::now().timestamp());
                        actor.service_status.healthy = true;
                        actor.service_status.error_count = 0;
                    } else {
                        actor.service_status.error_count += 1;
                        if actor.service_status.error_count >= 3 {
                            actor.service_status.healthy = false;
                        }
                    }
                }
                result
            }),
        )
    }
}

impl Handler<GetServiceStatus> for BlueskyActor {
    type Result = Result<ServiceStatus, NorppaliveError>;

    fn handle(&mut self, _msg: GetServiceStatus, _ctx: &mut Self::Context) -> Self::Result {
        Ok(self.service_status.clone())
    }
}
