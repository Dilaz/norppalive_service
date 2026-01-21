use actix::prelude::*;
use clap::Parser;
use miette::Result;
use tracing::{error, info};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

pub mod actors;
pub mod config;
pub mod error;
pub mod message_bus;
pub mod messages;
pub mod services;
pub mod utils;

use crate::actors::names::{
    BLUESKY_ACTOR, DETECTION_ACTOR, KAFKA_ACTOR, MASTODON_ACTOR, OUTPUT_ACTOR, STREAM_ACTOR,
    TWITTER_ACTOR,
};
use crate::config::{Service, CONFIG};
use crate::messages::supervisor::{RegisterActor, SystemShutdown};
use actors::services::ServiceActor;
use actors::{DetectionActor, OutputActor, StreamActor, SupervisorActor};
use messages::{GetSystemHealth, StartStream};
use services::{BlueskyService, KafkaService, MastodonService, TwitterService};
use utils::output::OutputService;

fn is_service_enabled(service: &Service) -> bool {
    CONFIG
        .output
        .services
        .iter()
        .any(|s| std::mem::discriminant(s) == std::mem::discriminant(service))
}

#[derive(Parser)]
#[command(version, about, long_about = None, name = "Norppalive Service", author = "Risto \"Dilaz\" Viitanen")]
struct Args {
    #[arg(short, long, default_value = "config.toml")]
    config: String,
}

fn main() -> Result<()> {
    // Parse CLI args and set CONFIG_PATH before any config access
    let args = Args::parse();
    std::env::set_var("CONFIG_PATH", &args.config);

    tracing::info!("Starting!");

    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "norppalive_service=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Create and run the actor system
    let system = System::new();

    system.block_on(async {
        // Start the supervisor
        let supervisor = SupervisorActor::new().start();

        // Start only the service actors that are configured
        let twitter_actor = if is_service_enabled(&Service::Twitter) {
            info!("Starting Twitter service");
            Some(ServiceActor::new(TwitterService).start())
        } else {
            None
        };
        let bluesky_actor = if is_service_enabled(&Service::Bluesky) {
            info!("Starting Bluesky service");
            Some(ServiceActor::new(BlueskyService::default()).start())
        } else {
            None
        };
        let mastodon_actor = if is_service_enabled(&Service::Mastodon) {
            info!("Starting Mastodon service");
            Some(ServiceActor::new(MastodonService).start())
        } else {
            None
        };
        let kafka_actor = if is_service_enabled(&Service::Kafka) {
            info!("Starting Kafka service");
            Some(ServiceActor::new(KafkaService::default()).start())
        } else {
            None
        };

        // Start OutputActor with service actors and supervisor for restart notifications
        let output_actor = OutputActor::with_services(
            Box::new(OutputService::default()),
            twitter_actor.clone(),
            bluesky_actor.clone(),
            mastodon_actor.clone(),
            kafka_actor.clone(),
            Some(supervisor.clone()),
        )
        .start();

        // Start DetectionActor with OutputActor
        let detection_actor = DetectionActor::with_output_actor(output_actor).start();

        // Start StreamActor
        let stream_actor =
            StreamActor::with_actors(detection_actor.clone(), supervisor.clone()).start();

        // Register all actors with supervisor for health monitoring
        // Service actors with factories for automatic restart
        use crate::messages::supervisor::ActorFactoryFn;
        use std::sync::Arc;

        // Only register service actors that are enabled in config
        if twitter_actor.is_some() {
            let twitter_factory: ActorFactoryFn = Arc::new(|| {
                Arc::new(ServiceActor::new(TwitterService).start())
            });
            supervisor.do_send(RegisterActor::with_factory(TWITTER_ACTOR, twitter_factory));
        }
        if bluesky_actor.is_some() {
            let bluesky_factory: ActorFactoryFn = Arc::new(|| {
                Arc::new(ServiceActor::new(BlueskyService::default()).start())
            });
            supervisor.do_send(RegisterActor::with_factory(BLUESKY_ACTOR, bluesky_factory));
        }
        if mastodon_actor.is_some() {
            let mastodon_factory: ActorFactoryFn = Arc::new(|| {
                Arc::new(ServiceActor::new(MastodonService).start())
            });
            supervisor.do_send(RegisterActor::with_factory(MASTODON_ACTOR, mastodon_factory));
        }
        if kafka_actor.is_some() {
            let kafka_factory: ActorFactoryFn = Arc::new(|| {
                Arc::new(ServiceActor::new(KafkaService::default()).start())
            });
            supervisor.do_send(RegisterActor::with_factory(KAFKA_ACTOR, kafka_factory));
        }

        // Register StreamActor with factory for auto-restart capability
        // The factory captures dependencies; ActorRestarted handler updates stale references
        let stream_factory: ActorFactoryFn = {
            let detection = detection_actor.clone();
            let sup = supervisor.clone();
            Arc::new(move || {
                Arc::new(StreamActor::with_actors(detection.clone(), sup.clone()).start())
            })
        };
        supervisor.do_send(RegisterActor::with_factory(STREAM_ACTOR, stream_factory));

        // Register other core actors without factories (complex dependencies make restart harder)
        supervisor.do_send(RegisterActor::new(DETECTION_ACTOR));
        supervisor.do_send(RegisterActor::new(OUTPUT_ACTOR));

        info!("Actor system started");

        // Verify system health
        if let Ok(health) = supervisor.send(GetSystemHealth).await {
            match health {
                Ok(h) if h.overall_healthy => info!("System health check passed"),
                Ok(h) => {
                    error!("System health check failed: restarts={}", h.total_restarts);
                    System::current().stop();
                    return;
                }
                Err(e) => {
                    error!("Failed to get system health: {}", e);
                    System::current().stop();
                    return;
                }
            }
        }

        // Start the stream processing
        info!("Starting stream processing");
        match stream_actor
            .send(StartStream {
                stream_url: CONFIG.stream.stream_url.clone(),
            })
            .await
        {
            Ok(Ok(())) => info!("Stream started successfully"),
            Ok(Err(e)) => {
                error!("Failed to start stream: {}", e);
                System::current().stop();
                return;
            }
            Err(e) => {
                error!("Failed to send start stream message: {}", e);
                System::current().stop();
                return;
            }
        }

        info!("Stream processing started. The actors will handle the video processing.");
        info!("The service will shut down automatically when the stream ends or an error occurs.");

        // Setup Ctrl+C handler for manual shutdown
        let supervisor_for_signal = supervisor.clone();
        actix::spawn(async move {
            match tokio::signal::ctrl_c().await {
                Ok(()) => {
                    info!("Received Ctrl+C signal, initiating manual shutdown...");
                    supervisor_for_signal.do_send(SystemShutdown);
                }
                Err(err) => {
                    error!("Unable to listen for shutdown signal: {}", err);
                    System::current().stop();
                }
            }
        });
    });

    // Run the system - this will block until System::current().stop() is called
    if let Err(e) = system.run() {
        error!("System run failed: {}", e);
        return Err(miette::miette!("System run failed: {}", e));
    }

    info!("Application shutdown complete.");
    Ok(())
}
