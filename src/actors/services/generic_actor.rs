//! Generic service actor that eliminates duplication across service implementations.

use actix::prelude::*;
use chrono::Utc;
use tracing::{error, info};

use crate::error::NorppaliveError;
use crate::messages::{GetServiceStatus, ServicePost, ServicePostResult, ServiceStatus};
use crate::services::SocialMediaService;

/// Generic actor for any service implementing SocialMediaService.
///
/// This eliminates the duplication between TwitterActor, BlueskyActor,
/// MastodonActor, and KafkaActor which all had ~90% identical code.
pub struct ServiceActor<S>
where
    S: SocialMediaService + Clone + Unpin + 'static,
{
    service: S,
    service_status: ServiceStatus,
}

impl<S> ServiceActor<S>
where
    S: SocialMediaService + Clone + Unpin + 'static,
{
    pub fn new(service: S) -> Self {
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

impl<S> Actor for ServiceActor<S>
where
    S: SocialMediaService + Clone + Unpin + 'static,
{
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        info!("ServiceActor ({}) started", self.service.name());
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!("ServiceActor ({}) stopped", self.service.name());
    }
}

impl<S> Handler<ServicePost> for ServiceActor<S>
where
    S: SocialMediaService + Clone + Unpin + 'static,
{
    type Result = ResponseActFuture<Self, Result<ServicePostResult, NorppaliveError>>;

    fn handle(&mut self, msg: ServicePost, _ctx: &mut Self::Context) -> Self::Result {
        let service_name = self.service.name().to_string();
        info!(
            "ServiceActor ({}) received post request: {}",
            service_name,
            msg.message.chars().take(50).collect::<String>()
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
                        actor.service_status.last_post_time = Some(post_result.posted_at);
                        actor.service_status.error_count = 0;
                        actor.service_status.healthy = true;
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

impl<S> Handler<GetServiceStatus> for ServiceActor<S>
where
    S: SocialMediaService + Clone + Unpin + 'static,
{
    type Result = Result<ServiceStatus, NorppaliveError>;

    fn handle(&mut self, _msg: GetServiceStatus, _ctx: &mut Self::Context) -> Self::Result {
        Ok(self.service_status.clone())
    }
}
