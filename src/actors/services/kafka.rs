use actix::prelude::*;
use chrono::Utc;
use tracing::{error, info};

use crate::error::NorppaliveError;
use crate::messages::{GetServiceStatus, ServicePost, ServicePostResult, ServiceStatus};
use crate::services::{ServiceType, SocialMediaService};

/// KafkaActor handles Kafka messaging
pub struct KafkaActor {
    service: ServiceType,
    service_status: ServiceStatus,
}

impl Actor for KafkaActor {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        info!("KafkaActor ({}) started", self.service.name());
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!("KafkaActor ({}) stopped", self.service.name());
    }
}

impl KafkaActor {
    pub fn new(service: ServiceType) -> Self {
        let service_name = service.name().to_string();
        Self {
            service,
            service_status: ServiceStatus {
                name: service_name,
                healthy: true,
                ..Default::default()
            },
        }
    }
}

impl Handler<ServicePost> for KafkaActor {
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
                        info!("Successfully posted message to {}", service_instance.name());
                        Ok(ServicePostResult {
                            success: true,
                            service_name: service_instance.name().to_string(),
                            error_message: None,
                            posted_at: Utc::now().timestamp(),
                        })
                    }
                    Err(err) => {
                        error!(
                            "Failed to post message to {}: {}",
                            service_instance.name(),
                            err
                        );
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
                match &result {
                    Ok(post_result) => {
                        if post_result.success {
                            actor.service_status.last_post_time = Some(post_result.posted_at);
                            actor.service_status.healthy = true;
                            actor.service_status.error_count = 0;
                        } else {
                            actor.service_status.error_count += 1;
                            // Set unhealthy if error count is high
                            if actor.service_status.error_count >= 3 {
                                actor.service_status.healthy = false;
                            }
                        }
                    }
                    Err(_) => {
                        actor.service_status.error_count += 1;
                        actor.service_status.healthy = false;
                    }
                }
                result
            }),
        )
    }
}

impl Handler<GetServiceStatus> for KafkaActor {
    type Result = Result<ServiceStatus, NorppaliveError>;

    fn handle(&mut self, _msg: GetServiceStatus, _ctx: &mut Self::Context) -> Self::Result {
        Ok(self.service_status.clone())
    }
}
