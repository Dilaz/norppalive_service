use actix::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::sync::oneshot;

/// Messages for SupervisorActor and system management

#[derive(Message)]
#[rtype(result = "()")]
pub struct ActorFailed {
    pub actor_name: String,
    pub error: String,
}

/// Message to request graceful shutdown of an actor.
/// The actor should finish its current work and then respond.
#[derive(Message)]
#[rtype(result = "()")]
pub struct GracefulStop {
    /// Channel to signal when the actor has finished cleanup
    pub ack_sender: Option<oneshot::Sender<()>>,
}

#[derive(Message)]
#[rtype(result = "Result<(), crate::error::NorppaliveError>")]
pub struct RestartActor {
    pub actor_name: String,
}

#[derive(Message)]
#[rtype(result = "Result<SystemHealth, crate::error::NorppaliveError>")]
pub struct GetSystemHealth;

#[derive(Message)]
#[rtype(result = "Result<(), crate::error::NorppaliveError>")]
pub struct ShutdownSystem;

#[derive(Message)]
#[rtype(result = "()")]
pub struct HealthCheck {
    pub actor_name: String,
    pub healthy: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemHealth {
    pub overall_healthy: bool,
    pub actor_statuses: HashMap<String, ActorHealth>,
    pub uptime_seconds: u64,
    pub total_restarts: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActorHealth {
    pub name: String,
    pub healthy: bool,
    pub last_restart: Option<i64>,
    pub restart_count: u32,
    pub error_count: u32,
    pub last_error: Option<String>,
}
