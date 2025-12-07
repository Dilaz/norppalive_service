use actix::prelude::*;
use chrono::Utc;
use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{error, info, warn};

use crate::error::NorppaliveError;
use crate::messages::supervisor::{
    ActorFactoryFn, ActorRestarted, RegisterActor, SubscribeToRestarts, SystemShutdown,
};
use crate::messages::{
    ActorFailed, ActorHealth, GetSystemHealth, HealthCheck, RestartActor, ShutdownSystem,
    SystemHealth,
};

/// Information about a managed actor including health and optional factory
struct ManagedActorInfo {
    health: ActorHealth,
    /// Optional factory for recreating the actor on restart
    factory: Option<ActorFactoryFn>,
    /// The current actor address (type-erased)
    address: Option<Arc<dyn Any + Send + Sync>>,
}

/// SupervisorActor manages the health and lifecycle of other actors in the system
pub struct SupervisorActor {
    start_time: Instant,
    /// Managed actors with health info and optional factories
    managed_actors: HashMap<String, ManagedActorInfo>,
    total_restarts: u32,
    shutdown_requested: bool,
    /// Subscribers to actor restart notifications
    restart_subscribers: Vec<Recipient<ActorRestarted>>,
}

impl Default for SupervisorActor {
    fn default() -> Self {
        Self {
            start_time: Instant::now(),
            managed_actors: HashMap::new(),
            total_restarts: 0,
            shutdown_requested: false,
            restart_subscribers: Vec::new(),
        }
    }
}

impl Actor for SupervisorActor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        info!("SupervisorActor started");

        // Schedule periodic health checks
        ctx.run_interval(Duration::from_secs(30), |actor, _ctx| {
            actor.perform_health_check();
        });
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!("SupervisorActor stopped. System will now terminate.");
    }
}

impl SupervisorActor {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a new actor for health monitoring with optional factory
    pub fn register_actor_with_factory(
        &mut self,
        name: String,
        factory: Option<ActorFactoryFn>,
        address: Option<Arc<dyn Any + Send + Sync>>,
    ) {
        let health = ActorHealth {
            name: name.clone(),
            healthy: true,
            last_restart: None,
            restart_count: 0,
            error_count: 0,
            last_error: None,
        };
        let info = ManagedActorInfo {
            health,
            factory,
            address,
        };
        self.managed_actors.insert(name.clone(), info);
        let has_factory = self
            .managed_actors
            .get(&name)
            .map(|i| i.factory.is_some())
            .unwrap_or(false);
        info!(
            "Registered actor '{}' for monitoring (restart capable: {})",
            name, has_factory
        );
    }

    /// Legacy method for backwards compatibility
    pub fn register_actor(&mut self, name: String) {
        self.register_actor_with_factory(name, None, None);
    }

    /// Subscribe to restart notifications
    pub fn subscribe_to_restarts(&mut self, subscriber: Recipient<ActorRestarted>) {
        self.restart_subscribers.push(subscriber);
    }

    /// Notify all subscribers about an actor restart
    fn notify_restart(&self, actor_name: &str, new_address: Arc<dyn Any + Send + Sync>) {
        let msg = ActorRestarted {
            actor_name: actor_name.to_string(),
            new_address,
        };
        for subscriber in &self.restart_subscribers {
            if let Err(e) = subscriber.try_send(msg.clone()) {
                warn!(
                    "Failed to notify subscriber about restart of '{}': {}",
                    actor_name, e
                );
            }
        }
    }

    /// Perform periodic health checks
    fn perform_health_check(&self) {
        let unhealthy_actors: Vec<_> = self
            .managed_actors
            .iter()
            .filter(|(_, info)| !info.health.healthy)
            .map(|(name, _)| name)
            .collect();

        if !unhealthy_actors.is_empty() {
            warn!("Unhealthy actors detected: {:?}", unhealthy_actors);
        }
    }

    /// Check if the system is overall healthy
    fn is_system_healthy(&self) -> bool {
        !self.shutdown_requested
            && self
                .managed_actors
                .values()
                .all(|info| info.health.healthy)
    }

    /// Get a map of actor health statuses (for GetSystemHealth response)
    fn get_actor_health_map(&self) -> HashMap<String, ActorHealth> {
        self.managed_actors
            .iter()
            .map(|(name, info)| (name.clone(), info.health.clone()))
            .collect()
    }

    /// Attempt to restart an actor using its factory
    fn restart_actor_internal(&mut self, actor_name: &str) -> Result<(), NorppaliveError> {
        let info = self.managed_actors.get_mut(actor_name).ok_or_else(|| {
            NorppaliveError::Other(format!("Unknown actor: {}", actor_name))
        })?;

        let factory = info.factory.as_ref().ok_or_else(|| {
            NorppaliveError::Other(format!(
                "Actor '{}' has no factory - cannot restart",
                actor_name
            ))
        })?;

        // Call the factory to create a new actor instance
        info!("Restarting actor '{}' using factory", actor_name);
        let new_address = factory();

        // Update the stored address
        info.address = Some(new_address.clone());

        // Update health status
        info.health.restart_count += 1;
        info.health.last_restart = Some(Utc::now().timestamp());
        info.health.healthy = true;
        info.health.last_error = None;

        self.total_restarts += 1;

        info!(
            "Actor '{}' restarted successfully (restart count: {})",
            actor_name, info.health.restart_count
        );

        // Notify subscribers about the restart
        self.notify_restart(actor_name, new_address);

        Ok(())
    }
}

impl Handler<ActorFailed> for SupervisorActor {
    type Result = ();

    fn handle(&mut self, msg: ActorFailed, _ctx: &mut Self::Context) -> Self::Result {
        error!("Actor '{}' failed: {}", msg.actor_name, msg.error);

        if let Some(info) = self.managed_actors.get_mut(&msg.actor_name) {
            info.health.healthy = false;
            info.health.error_count += 1;
            info.health.last_error = Some(msg.error.clone());

            // Attempt automatic restart if factory is available
            if info.factory.is_some() {
                info!(
                    "Actor '{}' has a factory, attempting automatic restart",
                    msg.actor_name
                );
                match self.restart_actor_internal(&msg.actor_name) {
                    Ok(()) => {
                        info!("Actor '{}' automatically restarted after failure", msg.actor_name);
                    }
                    Err(e) => {
                        error!(
                            "Failed to automatically restart actor '{}': {}",
                            msg.actor_name, e
                        );
                    }
                }
            } else {
                warn!(
                    "Actor '{}' marked as unhealthy (no factory for automatic restart)",
                    msg.actor_name
                );
            }
        }
    }
}

impl Handler<RestartActor> for SupervisorActor {
    type Result = Result<(), NorppaliveError>;

    fn handle(&mut self, msg: RestartActor, _ctx: &mut Self::Context) -> Self::Result {
        info!("Restart requested for actor '{}'", msg.actor_name);
        self.restart_actor_internal(&msg.actor_name)
    }
}

impl Handler<GetSystemHealth> for SupervisorActor {
    type Result = Result<SystemHealth, NorppaliveError>;

    fn handle(&mut self, _msg: GetSystemHealth, _ctx: &mut Self::Context) -> Self::Result {
        let uptime_seconds = self.start_time.elapsed().as_secs();
        let overall_healthy = self.is_system_healthy();

        Ok(SystemHealth {
            overall_healthy,
            actor_statuses: self.get_actor_health_map(),
            uptime_seconds,
            total_restarts: self.total_restarts,
        })
    }
}

impl Handler<ShutdownSystem> for SupervisorActor {
    type Result = Result<(), NorppaliveError>;

    fn handle(&mut self, _msg: ShutdownSystem, ctx: &mut Self::Context) -> Self::Result {
        info!("System shutdown requested");
        self.shutdown_requested = true;

        // Stop the actor system
        ctx.stop();
        System::current().stop();

        Ok(())
    }
}

impl Handler<HealthCheck> for SupervisorActor {
    type Result = ();

    fn handle(&mut self, msg: HealthCheck, _ctx: &mut Self::Context) -> Self::Result {
        if let Some(info) = self.managed_actors.get_mut(&msg.actor_name) {
            info.health.healthy = msg.healthy;
            if msg.healthy {
                info.health.last_error = None;
            }
        }
    }
}

impl Handler<RegisterActor> for SupervisorActor {
    type Result = ();

    fn handle(&mut self, msg: RegisterActor, _ctx: &mut Self::Context) -> Self::Result {
        self.register_actor_with_factory(msg.name, msg.factory, None);
    }
}

impl Handler<SubscribeToRestarts> for SupervisorActor {
    type Result = ();

    fn handle(&mut self, msg: SubscribeToRestarts, _ctx: &mut Self::Context) -> Self::Result {
        info!("New subscriber registered for actor restart notifications");
        self.restart_subscribers.push(msg.subscriber);
    }
}

impl Handler<SystemShutdown> for SupervisorActor {
    type Result = ();

    fn handle(&mut self, _msg: SystemShutdown, ctx: &mut Context<Self>) -> Self::Result {
        info!("SupervisorActor: Received SystemShutdown message. Initiating shutdown sequence.");

        // Schedule immediate shutdown
        info!("SupervisorActor: Stopping Actix system.");
        ctx.run_later(std::time::Duration::from_millis(100), |_actor, _ctx| {
            actix::System::current().stop();
        });

        // Log if graceful shutdown takes too long (but don't force exit to allow proper cleanup)
        ctx.run_later(std::time::Duration::from_secs(5), |_actor, _ctx| {
            warn!("SupervisorActor: Graceful shutdown taking longer than 5 seconds. Waiting for cleanup to complete...");
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::time::Duration;
    use tokio::time::sleep;

    #[actix::test]
    async fn test_supervisor_actor_creation() {
        let supervisor = SupervisorActor::new();
        assert_eq!(supervisor.total_restarts, 0);
        assert!(!supervisor.shutdown_requested);
        assert!(supervisor.managed_actors.is_empty());
    }

    #[actix::test]
    async fn test_supervisor_actor_startup() {
        let supervisor = SupervisorActor::new().start();

        // Test system health when no actors are registered
        let health = supervisor.send(GetSystemHealth).await.unwrap().unwrap();
        assert!(health.overall_healthy);
        assert_eq!(health.total_restarts, 0);
        assert!(health.actor_statuses.is_empty());
    }

    #[actix::test]
    async fn test_actor_registration_and_health() {
        let mut supervisor = SupervisorActor::new();

        // Register a test actor
        supervisor.register_actor("test_actor".to_string());

        // Check that the actor was registered
        assert!(supervisor.managed_actors.contains_key("test_actor"));
        let info = supervisor.managed_actors.get("test_actor").unwrap();
        assert!(info.health.healthy);
        assert_eq!(info.health.restart_count, 0);
        assert_eq!(info.health.error_count, 0);
        assert!(info.factory.is_none()); // No factory registered with legacy method
    }

    #[actix::test]
    async fn test_actor_registration_with_factory() {
        let supervisor = SupervisorActor::new().start();

        // Create a simple factory that returns a dummy value
        let factory: ActorFactoryFn = Arc::new(|| Arc::new(42i32));

        // Register with factory
        supervisor.do_send(RegisterActor::with_factory("test_actor", factory));

        // Small delay for message processing
        sleep(Duration::from_millis(10)).await;

        // Check system health
        let health = supervisor.send(GetSystemHealth).await.unwrap().unwrap();
        assert!(health.actor_statuses.contains_key("test_actor"));
    }

    #[actix::test]
    async fn test_actor_restart_with_factory() {
        let supervisor = SupervisorActor::new().start();

        // Create a factory that returns a counter value
        let counter = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let counter_clone = counter.clone();
        let factory: ActorFactoryFn = Arc::new(move || {
            let val = counter_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Arc::new(val)
        });

        // Register with factory
        supervisor.do_send(RegisterActor::with_factory("test_actor", factory));
        sleep(Duration::from_millis(10)).await;

        // Request restart - should now succeed
        let result = supervisor
            .send(RestartActor {
                actor_name: "test_actor".to_string(),
            })
            .await
            .unwrap();

        assert!(result.is_ok());

        // Factory should have been called
        assert_eq!(counter.load(std::sync::atomic::Ordering::SeqCst), 1);

        // Check restart count
        let health = supervisor.send(GetSystemHealth).await.unwrap().unwrap();
        assert_eq!(health.total_restarts, 1);
        let actor_health = health.actor_statuses.get("test_actor").unwrap();
        assert_eq!(actor_health.restart_count, 1);
    }

    #[actix::test]
    async fn test_actor_failure_handling() {
        let supervisor = SupervisorActor::new().start();

        // Register an actor first
        supervisor.do_send(RegisterActor::new("test_actor"));
        sleep(Duration::from_millis(10)).await;

        // Simulate actor failure
        supervisor
            .send(ActorFailed {
                actor_name: "test_actor".to_string(),
                error: "Test error".to_string(),
            })
            .await
            .unwrap();

        // Check system health after failure - no factory so no restart
        let health = supervisor.send(GetSystemHealth).await.unwrap().unwrap();
        assert_eq!(health.total_restarts, 0); // No restarts (no factory)

        // Actor should be marked unhealthy
        let actor_health = health.actor_statuses.get("test_actor").unwrap();
        assert!(!actor_health.healthy);
        assert_eq!(actor_health.error_count, 1);
    }

    #[actix::test]
    async fn test_actor_failure_with_factory_triggers_restart() {
        let supervisor = SupervisorActor::new().start();

        // Create a factory
        let restart_count = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let restart_count_clone = restart_count.clone();
        let factory: ActorFactoryFn = Arc::new(move || {
            restart_count_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Arc::new(())
        });

        // Register with factory
        supervisor.do_send(RegisterActor::with_factory("test_actor", factory));
        sleep(Duration::from_millis(10)).await;

        // Simulate actor failure - should trigger automatic restart
        supervisor
            .send(ActorFailed {
                actor_name: "test_actor".to_string(),
                error: "Test error".to_string(),
            })
            .await
            .unwrap();

        // Factory should have been called for restart
        assert_eq!(
            restart_count.load(std::sync::atomic::Ordering::SeqCst),
            1
        );

        // Check health - actor should be healthy again after restart
        let health = supervisor.send(GetSystemHealth).await.unwrap().unwrap();
        let actor_health = health.actor_statuses.get("test_actor").unwrap();
        assert!(actor_health.healthy);
        assert_eq!(actor_health.restart_count, 1);
    }

    #[actix::test]
    async fn test_actor_restart_without_factory() {
        let supervisor = SupervisorActor::new().start();

        // Register an actor without factory
        supervisor.do_send(RegisterActor::new("test_actor"));
        sleep(Duration::from_millis(10)).await;

        // Request restart - should fail because no factory
        let result = supervisor
            .send(RestartActor {
                actor_name: "test_actor".to_string(),
            })
            .await
            .unwrap();

        assert!(result.is_err());

        // Test system health
        let health = supervisor.send(GetSystemHealth).await.unwrap().unwrap();
        assert_eq!(health.total_restarts, 0); // No successful restarts
    }

    #[actix::test]
    async fn test_health_check_updates() {
        let supervisor = SupervisorActor::new().start();

        // First register the actor
        supervisor.do_send(RegisterActor::new("new_actor"));
        sleep(Duration::from_millis(10)).await;

        // Send health check update
        supervisor.do_send(HealthCheck {
            actor_name: "new_actor".to_string(),
            healthy: true,
        });
        sleep(Duration::from_millis(10)).await;

        // Send another health check with failure
        supervisor.do_send(HealthCheck {
            actor_name: "new_actor".to_string(),
            healthy: false,
        });
        sleep(Duration::from_millis(10)).await;

        // Check system health
        let health = supervisor.send(GetSystemHealth).await.unwrap().unwrap();
        let actor_health = health.actor_statuses.get("new_actor").unwrap();
        assert!(!actor_health.healthy);
    }

    #[actix::test]
    async fn test_system_shutdown() {
        let supervisor = SupervisorActor::new().start();

        // Request shutdown
        let result = supervisor.send(ShutdownSystem).await.unwrap();
        assert!(result.is_ok());
    }

    #[actix::test]
    async fn test_system_health_response_structure() {
        let supervisor = SupervisorActor::new().start();

        let health = supervisor.send(GetSystemHealth).await.unwrap().unwrap();

        // Verify all required fields are present
        assert!(health.overall_healthy);
        assert!(health.actor_statuses.is_empty()); // No actors registered yet
        assert_eq!(health.total_restarts, 0);
    }

    #[actix::test]
    async fn test_supervisor_shutdown() {
        let supervisor = SupervisorActor::new().start();
        supervisor.do_send(SystemShutdown);

        // Give some time for the system to stop
        sleep(Duration::from_millis(500)).await;

        // Verify the supervisor is still responsive after shutdown message
        let health_result = supervisor.send(GetSystemHealth).await;
        assert!(
            health_result.is_ok(),
            "Supervisor should still be responsive after shutdown message"
        );
    }
}
