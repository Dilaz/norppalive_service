//! Standalone CLI to post an image or video with a message to Bluesky,
//! Mastodon, and/or Discord (via the same Kafka topic the bot consumes).
//!
//! Reads the same `config.toml` as `norppalive_service`. Run with
//! `cargo run --bin poster -- --media path/to/file --message "..."`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use atrium_api::{
    agent::{atp_agent::store::MemorySessionStore, atp_agent::AtpAgent},
    com::atproto::repo::create_record::InputData,
    types::{
        string::{AtIdentifier, Handle, Nsid},
        BlobRef, DataModel, Object, TypedBlobRef, Unknown,
    },
};
use atrium_xrpc_client::reqwest::ReqwestClient;
use clap::Parser;
use ipld_core::ipld::Ipld;
use miette::Result;
use tokio::sync::Mutex;
use tracing::{error, info, warn};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use norppalive_service::config::{Service, CONFIG};
use norppalive_service::error::NorppaliveError;
use norppalive_service::services::{KafkaService, MastodonService, SocialMediaService};

const BLUESKY_COLLECTION_POST: &str = "app.bsky.feed.post";
const BLUESKY_COLLECTION_IMAGES: &str = "app.bsky.embed.images";
const BLUESKY_COLLECTION_VIDEO: &str = "app.bsky.embed.video";
const BLUESKY_BLOB_TYPE: &str = "blob";

/// Mirrors `MAX_IMAGE_BASE64_LEN` in `norppalive-discord/src/actors/discord.rs`.
/// Payloads larger than this are dropped by the bot.
const KAFKA_MAX_BASE64_LEN: usize = 14 * 1024 * 1024;

#[derive(Parser, Debug)]
#[command(
    version,
    about = "Post an image or video with a message to Bluesky, Mastodon, and/or Discord (via Kafka).",
    name = "norppalive-poster",
    author = "Norppalive"
)]
struct Args {
    /// Path to the config file (same format as norppalive_service config.toml).
    #[arg(short, long, default_value = "config.toml")]
    config: String,

    /// Image or video file to upload. Type is detected from the extension.
    /// Omit for a text-only post (Bluesky/Mastodon only; Kafka/Discord is skipped).
    #[arg(short = 'm', long)]
    media: Option<PathBuf>,

    /// Post text. If omitted, a random message from `[output] messages` is used.
    #[arg(short = 't', long)]
    message: Option<String>,

    /// Comma-separated services to post to: bluesky, mastodon, kafka.
    /// If omitted, uses the `[output] services` list from the config.
    #[arg(short, long, value_delimiter = ',')]
    services: Option<Vec<String>>,

    /// Alt text for Bluesky / Mastodon attachments.
    #[arg(short = 'a', long, default_value = "")]
    alt_text: String,

    /// Bluesky 2FA email code (also read from BLUESKY_AUTH_CODE).
    /// Required when the account has email-2FA enabled.
    #[arg(long)]
    bluesky_auth_code: Option<String>,
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('\\') => out.push('\\'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MediaKind {
    Image,
    Video,
}

fn detect_media_kind(path: &Path) -> MediaKind {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase());
    match ext.as_deref() {
        Some("mp4" | "mov" | "webm" | "mkv" | "avi" | "m4v") => MediaKind::Video,
        _ => MediaKind::Image,
    }
}

fn parse_services(args: &Args) -> Vec<Service> {
    if let Some(svcs) = &args.services {
        svcs.iter()
            .filter_map(|s| match s.trim().to_ascii_lowercase().as_str() {
                "" => None,
                "bluesky" | "bsky" => Some(Service::Bluesky),
                "mastodon" | "masto" => Some(Service::Mastodon),
                "kafka" | "discord" => Some(Service::Kafka),
                "twitter" => Some(Service::Twitter),
                other => {
                    warn!("Ignoring unknown service '{}'", other);
                    None
                }
            })
            .collect()
    } else {
        CONFIG.output.services.clone()
    }
}

fn resolve_message(args: &Args) -> String {
    args.message
        .clone()
        .or_else(|| CONFIG.output.get_random_message().cloned())
        .unwrap_or_else(|| String::from("Norppa on kivellä!"))
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    // Set CONFIG_PATH before any access to the lazy_static CONFIG.
    std::env::set_var("CONFIG_PATH", &args.config);

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let media = match args.media.clone() {
        Some(p) if !p.exists() => {
            error!("Media file not found: {}", p.display());
            std::process::exit(2);
        }
        Some(p) => {
            let kind = detect_media_kind(&p);
            Some((p, kind))
        }
        None => None,
    };

    let message = unescape(&resolve_message(&args));
    let services = parse_services(&args);

    let bluesky_auth_code = args.bluesky_auth_code.clone().or_else(|| {
        std::env::var("BLUESKY_AUTH_CODE")
            .ok()
            .filter(|s| !s.is_empty())
    });

    if services.is_empty() {
        error!("No services to post to. Configure `[output] services` or pass --services.");
        std::process::exit(2);
    }

    match &media {
        Some((p, kind)) => info!(
            "Posting {} '{}' with message {:?} to {:?}",
            match kind {
                MediaKind::Image => "image",
                MediaKind::Video => "video",
            },
            p.display(),
            message,
            services,
        ),
        None => info!("Posting text-only message {:?} to {:?}", message, services),
    }

    let mut had_failure = false;

    for svc in &services {
        let result = match svc {
            Service::Bluesky => {
                post_to_bluesky(
                    &message,
                    media.as_ref().map(|(p, k)| (p.as_path(), *k)),
                    &args.alt_text,
                    bluesky_auth_code.as_deref(),
                )
                .await
            }
            Service::Mastodon => {
                post_to_mastodon(&message, media.as_ref().map(|(p, _)| p.as_path())).await
            }
            Service::Kafka => match &media {
                Some((p, kind)) => post_to_kafka(&message, p, *kind).await,
                None => {
                    warn!(
                        "Skipping Kafka/Discord: no media supplied, and the Discord bot's \
                         consumer always builds an attachment from the payload."
                    );
                    Ok(())
                }
            },
            Service::Twitter => {
                warn!("Twitter is not handled by this poster; skipping.");
                Ok(())
            }
        };

        match result {
            Ok(()) => info!("[{:?}] Posted successfully.", svc),
            Err(e) => {
                error!("[{:?}] Failed: {}", svc, e);
                had_failure = true;
            }
        }
    }

    if had_failure {
        std::process::exit(1);
    }
    Ok(())
}

async fn post_to_kafka(
    message: &str,
    path: &Path,
    kind: MediaKind,
) -> std::result::Result<(), NorppaliveError> {
    let raw_len = std::fs::metadata(path)?.len() as usize;
    let base64_len_estimate = raw_len.div_ceil(3) * 4;
    if base64_len_estimate > KAFKA_MAX_BASE64_LEN {
        return Err(NorppaliveError::Other(format!(
            "Media is too large for the Discord bot consumer (~{} base64 bytes, max {}).",
            base64_len_estimate, KAFKA_MAX_BASE64_LEN
        )));
    }
    if matches!(kind, MediaKind::Video) {
        warn!(
            "Sending a video via Kafka. The Discord bot currently attaches every payload as \
             'image.jpg', so video previews may not render until the bot is updated."
        );
    }

    KafkaService::default()
        .post(message, &path.to_string_lossy())
        .await
}

async fn bluesky_login(
    agent: &Arc<Mutex<AtpAgent<MemorySessionStore, ReqwestClient>>>,
    auth_code: Option<&str>,
) -> std::result::Result<(), NorppaliveError> {
    if let Some(code) = auth_code {
        // 2FA path: call create_session directly with the email token,
        // then hand the resulting session back to the agent.
        let input = atrium_api::com::atproto::server::create_session::InputData {
            allow_takendown: None,
            auth_factor_token: Some(code.to_string()),
            identifier: CONFIG.bluesky.login.clone(),
            password: CONFIG.bluesky.password.clone(),
        };
        let g = agent.lock().await;
        let session = g
            .api
            .com
            .atproto
            .server
            .create_session(input.into())
            .await?;
        g.resume_session(session).await.map_err(|e| {
            NorppaliveError::Other(format!("Failed to resume Bluesky session: {e}"))
        })?;
        return Ok(());
    }

    let g = agent.lock().await;
    match g
        .login(&CONFIG.bluesky.login, &CONFIG.bluesky.password)
        .await
    {
        Ok(_) => Ok(()),
        Err(e) => {
            // Surface 2FA gating with a friendly hint instead of the raw 401.
            let s = format!("{e:?}");
            if s.contains("AuthFactorTokenRequired") {
                Err(NorppaliveError::Other(
                    "Bluesky requires a 2FA email code. Check your email and re-run with \
                     --bluesky-auth-code <code> (or set BLUESKY_AUTH_CODE)."
                        .into(),
                ))
            } else {
                Err(e.into())
            }
        }
    }
}

async fn post_to_mastodon(
    message: &str,
    media_path: Option<&Path>,
) -> std::result::Result<(), NorppaliveError> {
    let result = match media_path {
        Some(p) => MastodonService.post(message, &p.to_string_lossy()).await,
        None => post_to_mastodon_text_only(message).await,
    };
    match result {
        Ok(()) => Ok(()),
        Err(NorppaliveError::MegalodonError(boxed)) => {
            if let megalodon::error::Error::OwnError(own) = boxed.as_ref() {
                if own.status == Some(413) {
                    let raw_len = media_path
                        .and_then(|p| std::fs::metadata(p).ok())
                        .map(|m| m.len());
                    return Err(NorppaliveError::Other(format!(
                        "Mastodon instance rejected the upload as too large (HTTP 413).{} \
                         Most instances cap video uploads around 40 MB; either trim/transcode \
                         the file or post from an instance with a larger limit.",
                        raw_len
                            .map(|l| format!(" File is {} bytes.", l))
                            .unwrap_or_default(),
                    )));
                }
            }
            Err(NorppaliveError::MegalodonError(boxed))
        }
        Err(e) => Err(e),
    }
}

async fn post_to_mastodon_text_only(message: &str) -> std::result::Result<(), NorppaliveError> {
    info!("Logging in to Mastodon (text-only post)");
    let client = megalodon::generator(
        megalodon::SNS::Mastodon,
        (&CONFIG.mastodon.host).into(),
        Some((&CONFIG.mastodon.token).into()),
        None,
    )
    .expect("Could not create Mastodon client");
    let res = client.post_status(message.to_string(), None).await?;
    info!("Mastodon status posted: {:?}", res.json());
    Ok(())
}

async fn post_to_bluesky(
    message: &str,
    media: Option<(&Path, MediaKind)>,
    alt_text: &str,
    auth_code: Option<&str>,
) -> std::result::Result<(), NorppaliveError> {
    if CONFIG.bluesky.login.is_empty() || CONFIG.bluesky.password.is_empty() {
        return Err(NorppaliveError::Other(
            "Bluesky credentials are not configured (login/password)".into(),
        ));
    }

    let agent = Arc::new(Mutex::new(AtpAgent::new(
        ReqwestClient::new(&CONFIG.bluesky.host),
        MemorySessionStore::default(),
    )));

    bluesky_login(&agent, auth_code).await?;
    info!("Logged in to Bluesky");

    let embed_ipld = if let Some((path, kind)) = media {
        let bytes = std::fs::read(path)?;
        let upload = {
            let g = agent.lock().await;
            g.api.com.atproto.repo.upload_blob(bytes).await?
        };

        let (cid, mime_type, size) = match &upload.blob {
            BlobRef::Typed(TypedBlobRef::Blob(b)) => {
                (b.r#ref.0.to_string(), b.mime_type.clone(), b.size)
            }
            BlobRef::Untyped(_) => {
                return Err(NorppaliveError::Other(
                    "Bluesky blob upload did not return a typed blob with size".into(),
                ));
            }
        };
        info!(
            "Uploaded blob to Bluesky (mime={}, size={} bytes)",
            mime_type, size
        );

        let blob_ipld = Ipld::Map(BTreeMap::from([
            ("$type".into(), Ipld::String(BLUESKY_BLOB_TYPE.into())),
            ("size".into(), Ipld::Integer(size as i128)),
            (
                "ref".into(),
                Ipld::Map(BTreeMap::from([("$link".into(), Ipld::String(cid))])),
            ),
            ("mimeType".into(), Ipld::String(mime_type)),
        ]));

        Some(match kind {
            MediaKind::Image => {
                let image_object = Ipld::Map(BTreeMap::from([
                    ("alt".into(), Ipld::String(alt_text.into())),
                    ("image".into(), blob_ipld),
                ]));
                Ipld::Map(BTreeMap::from([
                    (
                        "$type".into(),
                        Ipld::String(BLUESKY_COLLECTION_IMAGES.into()),
                    ),
                    ("images".into(), Ipld::List(vec![image_object])),
                ]))
            }
            MediaKind::Video => Ipld::Map(BTreeMap::from([
                (
                    "$type".into(),
                    Ipld::String(BLUESKY_COLLECTION_VIDEO.into()),
                ),
                ("video".into(), blob_ipld),
                ("alt".into(), Ipld::String(alt_text.into())),
            ])),
        })
    } else {
        None
    };

    let post_type = DataModel::try_from(Ipld::String(BLUESKY_COLLECTION_POST.into())).unwrap();
    let text_data = DataModel::try_from(Ipld::String(message.into())).unwrap();
    let created_at = DataModel::try_from(Ipld::String(chrono::Utc::now().to_rfc3339())).unwrap();

    let mut record_fields = BTreeMap::from([
        ("$type".into(), post_type),
        ("text".into(), text_data),
        ("createdAt".into(), created_at),
    ]);
    if let Some(embed_ipld) = embed_ipld {
        record_fields.insert("embed".into(), DataModel::try_from(embed_ipld)?);
    }

    let record_data = Object::<InputData> {
        data: InputData {
            collection: Nsid::new(BLUESKY_COLLECTION_POST.into()).unwrap(),
            repo: AtIdentifier::Handle(Handle::new((&CONFIG.bluesky.handle).into()).unwrap()),
            record: Unknown::Object(record_fields),
            rkey: None,
            swap_commit: None,
            validate: None,
        },
        extra_data: Ipld::Null,
    };

    let agent = agent.lock().await;
    let res = agent
        .api
        .com
        .atproto
        .repo
        .create_record(record_data)
        .await?;
    info!("Posted to Bluesky: {}", res.uri);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_video_extensions() {
        for ext in ["mp4", "MOV", "webm", "mkv", "avi", "m4v"] {
            let p = PathBuf::from(format!("clip.{}", ext));
            assert_eq!(detect_media_kind(&p), MediaKind::Video, "ext={}", ext);
        }
    }

    #[test]
    fn unescape_handles_common_escapes() {
        assert_eq!(unescape(r"line1\nline2"), "line1\nline2");
        assert_eq!(unescape(r"a\tb"), "a\tb");
        assert_eq!(unescape(r"keep\\slash"), "keep\\slash");
        assert_eq!(unescape(r"unknown \q stays"), r"unknown \q stays");
        assert_eq!(unescape("plain text"), "plain text");
    }

    #[test]
    fn defaults_to_image() {
        for ext in ["jpg", "png", "webp", "JPG", ""] {
            let p = if ext.is_empty() {
                PathBuf::from("frame")
            } else {
                PathBuf::from(format!("frame.{}", ext))
            };
            assert_eq!(detect_media_kind(&p), MediaKind::Image, "ext={}", ext);
        }
    }
}
