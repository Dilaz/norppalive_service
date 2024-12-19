use atrium_xrpc_client::reqwest::ReqwestClient;
use atrium_api::{agent::{store::MemorySessionStore, AtpAgent}, com::atproto::repo::create_record::InputData, types::{string::{AtIdentifier, Handle, Nsid}, BlobRef, DataModel, Object, TypedBlobRef, Unknown}};
use ipld_core::ipld::Ipld;
use tracing::{debug, info, warn};

use crate::CONFIG;

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
    agent: AtpAgent<MemorySessionStore, ReqwestClient>,
    is_logged_in: bool,
}

impl SocialMediaService for BlueskyService {
    async fn post(&self, message: &str, image_path: &str) -> Result<(), String> {
        if !self.is_logged_in {
            warn!("Already logged in to Bluesky, logging in...");
            self.login().await?;
            info!("Logged in to Bluesky");
        }

        let blob_ref = self.upload_image(image_path).await?;

        // Post the message to Bluesky
        let record_data = Object::<InputData> {
            data: InputData { 
                collection: Nsid::new(BLUESKY_COLLECTION.into()).unwrap(),
                record: Unknown::Object({
                    std::collections::BTreeMap::from([
                        ("$type".to_string(), DataModel::try_from(Ipld::String(BLUESKY_COLLECTION.to_string())).unwrap()),
                        ("text".to_string(), DataModel::try_from(Ipld::String(message.into())).unwrap()),
                        ("createdAt".to_string(), DataModel::try_from(Ipld::String(chrono::Utc::now().to_rfc3339())).unwrap()),
                        ("embed".to_string(), DataModel::try_from(Ipld::Map({
                            std::collections::BTreeMap::from([
                                ("$type".to_string(), Ipld::String(BLUESKY_COLLECTION_IMAGE.to_string())),
                                ("images".to_string(), Ipld::List(vec![{
                                    Ipld::Map(std::collections::BTreeMap::from([
                                        ("$type".to_string(), Ipld::String(BLUESKY_BLOB_TYPE.to_string())),
                                        ("alt".to_string(), Ipld::String("".to_string())),
                                        ("image".to_string(), Ipld::Map({
                                            std::collections::BTreeMap::from([
                                                ("$type".to_string(), Ipld::String(BLUESKY_BLOB_TYPE.to_string())),
                                                ("size".to_string(), Ipld::Integer(blob_ref.size as i128)),
                                                ("ref".to_string(), Ipld::Map({
                                                    std::collections::BTreeMap::from([("$link".to_string(), Ipld::String(blob_ref.cid))])
                                                })),
                                                ("mimeType".to_string(), Ipld::String(blob_ref.mime_type)),
                                            ])
                                        })),
                                    ]))
                                }])),
                            ])
                        })).map_err(|err| std::format!("Error creating embed map {}", err))?),
                    ])
                }),
                repo: AtIdentifier::Handle(Handle::new((&CONFIG.bluesky.handle).into()).unwrap()),
                rkey: Option::None,
                swap_commit: Option::None,
                validate: Option::None,
            },
            extra_data: Ipld::Null,
        };

        debug!("Record data: {:?}", record_data);
        info!("Posting to Bluesky...");
        let post = self.agent.api.com.atproto.repo.create_record(record_data).await
            .map_err(|err| format!("Error posting to Bluesky: {:?}", err))?;
        info!("Posted to Bluesky: {}", post.uri);
        debug!("Post: {:?}", post);

        Ok(())
    }
}

impl Default for BlueskyService {
    fn default() -> Self {
        Self {
            agent: AtpAgent::new(
                ReqwestClient::new(&CONFIG.bluesky.host),
                MemorySessionStore::default(),
            ),
            is_logged_in: false,
        }
    }
}
    
impl BlueskyService {
    async fn login(&self) -> Result<(), String> {
        let _login = self.agent.login(&CONFIG.bluesky.login, &CONFIG.bluesky.password).await
            .map_err(|err| format!("Error logging in to Bluesky: {:?}", err))?;

        Ok(())
    }

    async fn upload_image(&self, image_path: &str) -> Result<UploadResponse, String> {
        info!("Uploading image: {}", image_path);
        let image_data = std::fs::read(image_path)
            .map_err(|err| format!("Error reading image file: {:?}", err))?;
    
        let res = self.agent.api.com.atproto.repo.upload_blob(image_data).await
            .map_err(|err| format!("Error creating blob: {:?}", err))?;

        debug!("Blob: {:?}", res);
        info!("Uploaded image to Bluesky");

        match &res.blob {
            BlobRef::Typed(TypedBlobRef::Blob(blob)) => Ok(UploadResponse {
                cid: blob.r#ref.0.to_string(),
                mime_type: blob.mime_type.clone(),
                size: blob.size,
            }),
            BlobRef::Untyped(blob) => Ok(UploadResponse {
                cid: blob.cid.clone(),
                mime_type: blob.mime_type.clone(),
                size: 0,
            }),
        }
    }
}
