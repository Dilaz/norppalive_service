use actix::prelude::*;
use chrono::Utc;
use tracing::{error, info};

use crate::error::NorppaliveError;
use crate::messages::{GetServiceStatus, ServicePost, ServicePostResult, ServiceStatus};
use crate::services::{KafkaService, SocialMediaService};

/// KafkaActor handles Kafka messaging
pub struct KafkaActor {
    service_status: ServiceStatus,
    kafka_service: KafkaService,
}

impl Default for KafkaActor {
    fn default() -> Self {
        Self {
            service_status: ServiceStatus {
                name: "Kafka".to_string(),
                healthy: true,
                last_post_time: None,
                error_count: 0,
                rate_limited: false,
            },
            kafka_service: KafkaService::default(),
        }
    }
}

impl Actor for KafkaActor {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        info!(
            "KafkaActor started with topic: {}",
            &self.kafka_service.topic
        );
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!("KafkaActor stopped");
    }
}

impl KafkaActor {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Handler<ServicePost> for KafkaActor {
    type Result = ResponseActFuture<Self, Result<ServicePostResult, NorppaliveError>>;

    fn handle(&mut self, msg: ServicePost, _ctx: &mut Self::Context) -> Self::Result {
        info!("Received Kafka message request: {}", msg.message);

        let message = msg.message.clone();
        let image_path = msg.image_path.clone();
        let kafka_service = self.kafka_service.clone();

        Box::pin(
            async move {
                match kafka_service.post(&message, &image_path).await {
                    Ok(_) => {
                        info!("Successfully posted message to Kafka");
                        Ok(ServicePostResult {
                            success: true,
                            service_name: "Kafka".to_string(),
                            error_message: None,
                            posted_at: Utc::now().timestamp(),
                        })
                    }
                    Err(err) => {
                        error!("Failed to post message to Kafka: {}", err);
                        Ok(ServicePostResult {
                            success: false,
                            service_name: "Kafka".to_string(),
                            error_message: Some(err.to_string()),
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
