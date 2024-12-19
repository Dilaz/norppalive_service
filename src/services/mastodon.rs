use megalodon::{entities::{StatusVisibility, UploadMedia}, megalodon::PostStatusInputOptions};
use tracing::{debug, info};

use crate::CONFIG;

use super::SocialMediaService;

#[derive(Debug, Default)]
pub struct MastodonService;

impl SocialMediaService for MastodonService {
    async fn post(&self, message: &str, image_path: &str) -> Result<(), String> {
        info!("Logging in to Mastodon");
        let client = megalodon::generator(
            megalodon::SNS::Mastodon,
            (&CONFIG.mastodon.host).into(),
            Some((&CONFIG.mastodon.token).into()),
            None,
        ).expect("Could not create Mastodon client");
    
        // Upload image to mastodon
        let upload_media = client.upload_media(image_path.to_string(), None).await
            .map_err(|err| format!("Error uploading image to Mastodon: {}", err))?
            .json();
        
        let options = PostStatusInputOptions {
            media_ids: Some(vec![
                match upload_media {
                    UploadMedia::Attachment(attachment) => attachment.id,
                    UploadMedia::AsyncAttachment(attachment) => attachment.id,
                }
            ]),
            poll: None,
            in_reply_to_id: None,
            sensitive: None,
            spoiler_text: None,
            visibility: Some(StatusVisibility::Public),
            scheduled_at: None,
            language: None,
            quote_id: None,
        };
    
        let res = client.post_status(message.to_string(), Some(&options)).await
        .map_err(|err| format!("Could not verity Mastodon login credentials: {}", err))?;
    
        info!("Logged in to Mastodon");
        debug!("{:#?}", res.json());
    
        Ok(())
    }
}
