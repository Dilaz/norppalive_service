use actix::prelude::*;

/// Message to tell the SupervisorActor to shut down the system.
#[derive(Message)]
#[rtype(result = "()")]
pub struct SystemShutdown;

/// Message to register an actor with the supervisor for health monitoring
#[derive(Message)]
#[rtype(result = "()")]
pub struct RegisterActor {
    pub name: String,
}
