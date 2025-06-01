use atrium_api::{
    agent::{atp_agent::store::MemorySessionStore, atp_agent::AtpAgent},
    com::atproto::repo::create_record::InputData,
    types::{
        string::{AtIdentifier, Handle, Nsid},
        BlobRef, DataModel, Object, TypedBlobRef, Unknown,
    },
};
use atrium_xrpc_client::reqwest::ReqwestClient;
use ipld_core::ipld::Ipld;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, info};

use crate::{config::CONFIG, error::NorppaliveError};

use super::SocialMediaService;

const BLUESKY_COLLECTION: &str = "app.bsky.feed.post";
const BLUESKY_COLLECTION_IMAGE: &str = "app.bsky.embed.images";
const BLUESKY_BLOB_TYPE: &str = "blob";

struct UploadResponse {
    cid: String,
    mime_type: String,
    size: usize,
}

pub struct BlueskyService {
    agent: Arc<Mutex<AtpAgent<MemorySessionStore, ReqwestClient>>>,
    is_logged_in: Arc<Mutex<bool>>,
}

impl SocialMediaService for BlueskyService {
    async fn post(&self, message: &str, image_path: &str) -> Result<(), NorppaliveError> {
        // Check if we need to login
        {
            let is_logged_in = self.is_logged_in.lock().await;
            if !*is_logged_in {
                drop(is_logged_in); // Release the lock before calling login
                self.login().await?;
                info!("Logged in to Bluesky");
            }
        }

        let blob_ref = self.upload_image(image_path).await?;

        // Post the message to Bluesky
        let record_data = Object::<InputData> {
            data: InputData {
                collection: Nsid::new(BLUESKY_COLLECTION.into()).unwrap(),
                repo: AtIdentifier::Handle(Handle::new((&CONFIG.bluesky.handle).into()).unwrap()),
                record: Unknown::Object({
                    let post_type =
                        DataModel::try_from(Ipld::String(BLUESKY_COLLECTION.to_string())).unwrap();
                    let text = DataModel::try_from(Ipld::String(message.into())).unwrap();
                    let created_at =
                        DataModel::try_from(Ipld::String(chrono::Utc::now().to_rfc3339())).unwrap();

                    // Correct blob structure for image
                    let actual_blob_ipld = Ipld::Map({
                        std::collections::BTreeMap::from([
                            (
                                "$type".to_string(),
                                Ipld::String(BLUESKY_BLOB_TYPE.to_string()),
                            ),
                            ("size".to_string(), Ipld::Integer(blob_ref.size as i128)),
                            (
                                "ref".to_string(),
                                Ipld::Map({
                                    std::collections::BTreeMap::from([(
                                        "$link".to_string(),
                                        Ipld::String(blob_ref.cid.clone()),
                                    )])
                                }),
                            ),
                            (
                                "mimeType".to_string(),
                                Ipld::String(blob_ref.mime_type.clone()),
                            ),
                        ])
                    });

                    // The app.bsky.embed.images#image object (no $type)
                    let image_object_for_array = Ipld::Map({
                        std::collections::BTreeMap::from([
                            ("alt".to_string(), Ipld::String("".to_string())),
                            ("image".to_string(), actual_blob_ipld),
                        ])
                    });

                    let images_list = Ipld::List(vec![image_object_for_array]);

                    let embed_map = DataModel::try_from(Ipld::Map({
                        std::collections::BTreeMap::from([
                            (
                                "$type".to_string(),
                                Ipld::String(BLUESKY_COLLECTION_IMAGE.to_string()),
                            ),
                            ("images".to_string(), images_list),
                        ])
                    }))?;

                    std::collections::BTreeMap::from([
                        ("$type".to_string(), post_type),
                        ("text".to_string(), text),
                        ("createdAt".to_string(), created_at),
                        ("embed".to_string(), embed_map),
                    ])
                }),
                rkey: Option::None,
                swap_commit: Option::None,
                validate: Option::None,
            },
            extra_data: Ipld::Null,
        };

        debug!("Record data: {:?}", record_data);
        info!("Posting to Bluesky...");

        let agent = self.agent.lock().await;
        let post = agent
            .api
            .com
            .atproto
            .repo
            .create_record(record_data)
            .await?;

        info!("Posted to Bluesky: {}", post.uri);
        debug!("Post: {:?}", post);

        Ok(())
    }

    fn name(&self) -> &'static str {
        "Bluesky"
    }
}

impl Clone for BlueskyService {
    fn clone(&self) -> Self {
        Self {
            agent: self.agent.clone(),
            is_logged_in: self.is_logged_in.clone(),
        }
    }
}

impl Default for BlueskyService {
    fn default() -> Self {
        Self {
            agent: Arc::new(Mutex::new(AtpAgent::new(
                ReqwestClient::new(&CONFIG.bluesky.host),
                MemorySessionStore::default(),
            ))),
            is_logged_in: Arc::new(Mutex::new(false)),
        }
    }
}

impl BlueskyService {
    async fn login(&self) -> Result<(), NorppaliveError> {
        let agent = self.agent.lock().await;
        let _login = agent
            .login(&CONFIG.bluesky.login, &CONFIG.bluesky.password)
            .await?;

        drop(agent); // Release the lock before updating is_logged_in

        let mut is_logged_in = self.is_logged_in.lock().await;
        *is_logged_in = true;

        Ok(())
    }

    async fn upload_image(&self, image_path: &str) -> Result<UploadResponse, NorppaliveError> {
        info!("Uploading image: {}", image_path);
        let image_data = std::fs::read(image_path)?;

        let agent = self.agent.lock().await;
        let res = agent.api.com.atproto.repo.upload_blob(image_data).await?;

        debug!("Blob: {:?}", res);
        info!("Uploaded image to Bluesky");

        match &res.blob {
            BlobRef::Typed(TypedBlobRef::Blob(blob)) => Ok(UploadResponse {
                cid: blob.r#ref.0.to_string(),
                mime_type: blob.mime_type.clone(),
                size: blob.size,
            }),
            BlobRef::Untyped(_) => {
                // Treat as error: we require a valid size for the blob
                Err(NorppaliveError::Other(
                    "Bluesky blob upload did not return a typed blob with size".to_string(),
                ))
            }
        }
    }
}
