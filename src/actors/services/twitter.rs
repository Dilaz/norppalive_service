use actix::prelude::*;
use chrono::Utc;
use std::time::{Duration, Instant};
use tracing::{error, info, warn};

use crate::error::NorppaliveError;
use crate::messages::{GetServiceStatus, ServicePost, ServicePostResult, ServiceStatus};
use crate::services::{SocialMediaService, TwitterService};

/// TwitterActor handles posting to Twitter
pub struct TwitterActor {
    service_status: ServiceStatus,
    last_post_time: Option<Instant>,
    rate_limit_reset: Option<Instant>,
}

impl Default for TwitterActor {
    fn default() -> Self {
        Self {
            service_status: ServiceStatus {
                name: "Twitter".to_string(),
                healthy: true,
                last_post_time: None,
                error_count: 0,
                rate_limited: false,
            },
            last_post_time: None,
            rate_limit_reset: None,
        }
    }
}

impl Actor for TwitterActor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        info!("TwitterActor started");

        // Schedule periodic health checks
        ctx.run_interval(Duration::from_secs(60), |actor, _ctx| {
            actor.check_rate_limit_status();
        });
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!("TwitterActor stopped");
    }
}

impl TwitterActor {
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if we're still rate limited
    fn check_rate_limit_status(&mut self) {
        if let Some(reset_time) = self.rate_limit_reset {
            if Instant::now() > reset_time {
                self.rate_limit_reset = None;
                self.service_status.rate_limited = false;
                info!("Twitter rate limit has been reset");
            }
        }
    }

    /// Check if enough time has passed since last post to avoid rate limiting
    fn can_post(&self) -> bool {
        if self.service_status.rate_limited {
            return false;
        }

        // Twitter allows 50 tweets per 24 hours for normal accounts
        // This means roughly one tweet every 29 minutes to be safe
        if let Some(last_post) = self.last_post_time {
            let min_interval = Duration::from_secs(30 * 60); // 30 minutes
            if Instant::now().duration_since(last_post) < min_interval {
                return false;
            }
        }

        true
    }

    /// Handle posting errors and update status
    #[allow(dead_code)]
    fn handle_post_error(&mut self, error: &NorppaliveError) {
        self.service_status.error_count += 1;
        self.service_status.healthy = false;

        // Check if it's a rate limit error
        let error_str = error.to_string().to_lowercase();
        if error_str.contains("rate limit") || error_str.contains("too many requests") {
            self.service_status.rate_limited = true;
            // Set rate limit reset time to 1 hour from now
            self.rate_limit_reset = Some(Instant::now() + Duration::from_secs(3600));
            warn!("Twitter rate limit detected, will retry in 1 hour");
        }

        error!("Twitter posting failed: {}", error);
    }
}

impl Handler<ServicePost> for TwitterActor {
    type Result = ResponseActFuture<Self, Result<ServicePostResult, NorppaliveError>>;

    fn handle(&mut self, msg: ServicePost, _ctx: &mut Self::Context) -> Self::Result {
        info!("Received Twitter post request: {}", msg.message);

        // Check rate limiting first, regardless of mock or real service
        if !self.can_post() {
            let error_msg = if self.service_status.rate_limited {
                "Twitter service is rate limited".to_string()
            } else {
                "Too soon since last Twitter post".to_string()
            };

            warn!("{}", error_msg);
            return Box::pin(actix::fut::ready(Ok(ServicePostResult {
                success: false,
                service_name: "Twitter".to_string(),
                error_message: Some(error_msg),
                posted_at: Utc::now().timestamp(),
            })));
        }

        let message = msg.message.clone();
        let image_path = msg.image_path.clone();

        Box::pin(
            async move {
                // Use real Twitter service in production
                let twitter_service = TwitterService;
                match twitter_service.post(&message, &image_path).await {
                    Ok(_) => {
                        info!("Successfully posted to Twitter");
                        Ok(ServicePostResult {
                            success: true,
                            service_name: "Twitter".to_string(),
                            error_message: None,
                            posted_at: Utc::now().timestamp(),
                        })
                    }
                    Err(err) => {
                        error!("Failed to post to Twitter: {}", err);
                        Ok(ServicePostResult {
                            success: false,
                            service_name: "Twitter".to_string(),
                            error_message: Some(err.to_string()),
                            posted_at: Utc::now().timestamp(),
                        })
                    }
                }
            }
            .into_actor(self)
            .map(|result, actor, _ctx| {
                // Update actor state based on result
                if let Ok(ref post_result) = result {
                    if post_result.success {
                        actor.last_post_time = Some(Instant::now());
                        actor.service_status.last_post_time = Some(Utc::now().timestamp());
                        actor.service_status.healthy = true;
                    } else {
                        actor.service_status.error_count += 1;
                        actor.service_status.healthy = false;

                        // Check for rate limiting
                        if let Some(ref error_msg) = post_result.error_message {
                            let error_str = error_msg.to_lowercase();
                            if error_str.contains("rate limit")
                                || error_str.contains("too many requests")
                            {
                                actor.service_status.rate_limited = true;
                                actor.rate_limit_reset =
                                    Some(Instant::now() + Duration::from_secs(3600));
                                warn!("Twitter rate limit detected, will retry in 1 hour");
                            }
                        }
                    }
                }
                result
            }),
        )
    }
}

impl Handler<GetServiceStatus> for TwitterActor {
    type Result = Result<ServiceStatus, NorppaliveError>;

    fn handle(&mut self, _msg: GetServiceStatus, _ctx: &mut Self::Context) -> Self::Result {
        Ok(self.service_status.clone())
    }
}
