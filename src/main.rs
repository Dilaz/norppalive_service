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

use crate::config::CONFIG;
use crate::messages::supervisor::{RegisterActor, SystemShutdown};
use actors::services::{BlueskyActor, KafkaActor, MastodonActor, TwitterActor};
use actors::{DetectionActor, OutputActor, StreamActor, SupervisorActor};
use messages::{GetSystemHealth, StartStream};
use services::{BlueskyService, KafkaService, MastodonService, ServiceType, TwitterService};
use utils::output::OutputService;

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

        // Start service actors
        let twitter_actor =
            TwitterActor::new(ServiceType::TwitterService(TwitterService)).start();
        let bluesky_actor =
            BlueskyActor::new(ServiceType::BlueskyService(BlueskyService::default())).start();
        let mastodon_actor =
            MastodonActor::new(ServiceType::MastodonService(MastodonService)).start();
        let kafka_actor =
            KafkaActor::new(ServiceType::KafkaService(KafkaService::default())).start();

        // Start OutputActor with service actors
        let output_actor = OutputActor::with_services(
            Box::new(OutputService::default()),
            Some(twitter_actor),
            Some(bluesky_actor),
            Some(mastodon_actor),
            Some(kafka_actor),
        )
        .start();

        // Start DetectionActor with OutputActor
        let detection_actor = DetectionActor::with_output_actor(output_actor).start();

        // Start StreamActor
        let stream_actor =
            StreamActor::with_actors(detection_actor.clone(), supervisor.clone()).start();

        // Register all actors with supervisor for health monitoring
        supervisor.do_send(RegisterActor {
            name: "StreamActor".to_string(),
        });
        supervisor.do_send(RegisterActor {
            name: "DetectionActor".to_string(),
        });
        supervisor.do_send(RegisterActor {
            name: "OutputActor".to_string(),
        });
        supervisor.do_send(RegisterActor {
            name: "TwitterActor".to_string(),
        });
        supervisor.do_send(RegisterActor {
            name: "BlueskyActor".to_string(),
        });
        supervisor.do_send(RegisterActor {
            name: "MastodonActor".to_string(),
        });
        supervisor.do_send(RegisterActor {
            name: "KafkaActor".to_string(),
        });

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
