use actix::prelude::*;
use chrono::Utc;
use tracing::{error, info};

use crate::error::NorppaliveError;
use crate::messages::{GetServiceStatus, ServicePost, ServicePostResult, ServiceStatus};
use crate::services::{ServiceType, SocialMediaService};

/// MastodonActor handles posting to Mastodon
pub struct MastodonActor {
    service: ServiceType,
    service_status: ServiceStatus,
}

impl Actor for MastodonActor {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        info!("MastodonActor ({}) started", self.service.name());
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!("MastodonActor ({}) stopped", self.service.name());
    }
}

impl MastodonActor {
    pub fn new(service: ServiceType) -> Self {
        let service_name = service.name().to_string();
        Self {
            service,
            service_status: ServiceStatus {
                name: service_name,
                healthy: true,
                last_post_time: None,
                error_count: 0,
                rate_limited: false,
            },
        }
    }
}

impl Handler<ServicePost> for MastodonActor {
    type Result = ResponseActFuture<Self, Result<ServicePostResult, NorppaliveError>>;

    fn handle(&mut self, msg: ServicePost, _ctx: &mut Self::Context) -> Self::Result {
        info!(
            "Received post request for {}: {}",
            self.service.name(),
            msg.message
        );

        let message = msg.message.clone();
        let image_path = msg.image_path.clone();
        let service_instance = self.service.clone();

        Box::pin(
            async move {
                match service_instance.post(&message, &image_path).await {
                    Ok(_) => {
                        info!("Successfully posted to {}", service_instance.name());
                        Ok(ServicePostResult {
                            success: true,
                            service_name: service_instance.name().to_string(),
                            error_message: None,
                            posted_at: Utc::now().timestamp(),
                        })
                    }
                    Err(err) => {
                        error!("Failed to post to {}: {}", service_instance.name(), err);
                        Ok(ServicePostResult {
                            success: false,
                            service_name: service_instance.name().to_string(),
                            error_message: Some(err.clone_for_error_reporting().to_string()),
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

impl Handler<GetServiceStatus> for MastodonActor {
    type Result = Result<ServiceStatus, NorppaliveError>;

    fn handle(&mut self, _msg: GetServiceStatus, _ctx: &mut Self::Context) -> Self::Result {
        Ok(self.service_status.clone())
    }
}
