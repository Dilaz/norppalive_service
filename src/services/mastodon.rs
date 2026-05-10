use std::time::Duration;

use async_trait::async_trait;
use megalodon::{
    entities::{StatusVisibility, UploadMedia},
    megalodon::PostStatusInputOptions,
};
use tokio::time::{sleep, Instant};
use tracing::{debug, info, warn};

use crate::{config::CONFIG, error::NorppaliveError};

use super::SocialMediaService;

const MEDIA_PROCESSING_TIMEOUT: Duration = Duration::from_secs(60);
const MEDIA_PROCESSING_POLL_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Debug, Default, Clone)]
pub struct MastodonService;

#[async_trait]
impl SocialMediaService for MastodonService {
    async fn post(&self, message: &str, image_path: &str) -> Result<(), NorppaliveError> {
        info!("Logging in to Mastodon");
        let client = megalodon::generator(
            megalodon::SNS::Mastodon,
            (&CONFIG.mastodon.host).into(),
            Some((&CONFIG.mastodon.token).into()),
            None,
        )
        .expect("Could not create Mastodon client");

        // Upload image to mastodon
        let upload_media = client
            .upload_media(image_path.to_string(), None)
            .await?
            .json();

        let media_id = match upload_media {
            UploadMedia::Attachment(attachment) => attachment.id,
            UploadMedia::AsyncAttachment(attachment) => {
                let id = attachment.id.clone();
                wait_for_media_ready(&client, &id).await?;
                id
            }
        };

        let options = PostStatusInputOptions {
            media_ids: Some(vec![media_id]),
            poll: None,
            in_reply_to_id: None,
            sensitive: None,
            spoiler_text: None,
            visibility: Some(StatusVisibility::Public),
            scheduled_at: None,
            language: None,
            quote_id: None,
        };

        let res = client
            .post_status(message.to_string(), Some(&options))
            .await?;

        info!("Logged in to Mastodon");
        debug!("{:#?}", res.json());

        Ok(())
    }

    fn name(&self) -> &'static str {
        "Mastodon"
    }
}

// Mastodon returns HTTP 206 from GET /api/v1/media/:id while the upload is
// still being transcoded, and 200 once the attachment is usable. Posting a
// status with an unprocessed media id yields a 422 ("Cannot attach files that
// have not finished processing").
async fn wait_for_media_ready(
    client: &Box<dyn megalodon::megalodon::Megalodon + Send + Sync>,
    media_id: &str,
) -> Result<(), NorppaliveError> {
    let deadline = Instant::now() + MEDIA_PROCESSING_TIMEOUT;
    loop {
        let res = client.get_media(media_id.to_string()).await?;
        if res.status == 200 {
            debug!("Mastodon media {} ready", media_id);
            return Ok(());
        }
        if Instant::now() >= deadline {
            warn!(
                "Mastodon media {} still processing after {:?}; giving up",
                media_id, MEDIA_PROCESSING_TIMEOUT
            );
            return Err(NorppaliveError::Other(format!(
                "Mastodon media {} did not finish processing within {:?}",
                media_id, MEDIA_PROCESSING_TIMEOUT
            )));
        }
        debug!(
            "Mastodon media {} still processing (status {}); polling again",
            media_id, res.status
        );
        sleep(MEDIA_PROCESSING_POLL_INTERVAL).await;
    }
}
