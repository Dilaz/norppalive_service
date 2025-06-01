use actix::prelude::*;

/// Message to tell the SupervisorActor to shut down the system.
#[derive(Message)]
#[rtype(result = "()")]
pub struct SystemShutdown;
