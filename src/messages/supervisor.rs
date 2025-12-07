use actix::prelude::*;
use std::any::Any;
use std::sync::Arc;

/// Message to tell the SupervisorActor to shut down the system.
#[derive(Message)]
#[rtype(result = "()")]
pub struct SystemShutdown;

/// A factory function that creates and starts an actor, returning an Arc-wrapped Any
/// containing the actor's address.
pub type ActorFactoryFn = Arc<dyn Fn() -> Arc<dyn Any + Send + Sync> + Send + Sync>;

/// Message to register an actor with the supervisor for health monitoring
/// Optionally includes a factory for restart capability
#[derive(Message)]
#[rtype(result = "()")]
pub struct RegisterActor {
    pub name: String,
    /// Optional factory function for restarting the actor
    pub factory: Option<ActorFactoryFn>,
}

impl RegisterActor {
    /// Create a registration without a factory (no automatic restart)
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            factory: None,
        }
    }

    /// Create a registration with a factory for automatic restart
    pub fn with_factory(name: impl Into<String>, factory: ActorFactoryFn) -> Self {
        Self {
            name: name.into(),
            factory: Some(factory),
        }
    }
}

/// Message sent when an actor has been successfully restarted
/// Subscribers can use this to update their references
#[derive(Message, Clone)]
#[rtype(result = "()")]
pub struct ActorRestarted {
    pub actor_name: String,
    /// The new actor address as a boxed Any
    pub new_address: Arc<dyn Any + Send + Sync>,
}
