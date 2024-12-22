use std::path::PathBuf;
use tracing::{error, info};
use twitter_api_v1::{endpoints::{media::upload_media::upload_image_from_file, EndpointRet}, objects::MediaCategory, TokenSecrets};
use twitter_v2::{authorization::Oauth1aToken, id::NumericId, TwitterApi};

use crate::{error::NorppaliveError, CONFIG};

use super::SocialMediaService;

#[derive(Debug, Default)]
pub struct TwitterService;

impl std::fmt::Display for TwitterService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TwitterService")
    }
}

impl SocialMediaService for TwitterService {
    async fn post(&self, message: &str, image_path: &str) -> Result<(), NorppaliveError> {
        info!("Posting to Twitter");
        let auth_token = Oauth1aToken::new(&CONFIG.twitter.consumer_key, &CONFIG.twitter.consumer_secret, &CONFIG.twitter.token, &CONFIG.twitter.token_secret);
        info!("Auth token created, uploading the image");
        let media_id = self.upload_image_from_file(image_path).await?;
        info!("Image uploaded to Twitter with media id {}", media_id);
    
        let mut twitter_api = TwitterApi::new(auth_token).post_tweet();
        let tweet = twitter_api
            .text(message.to_string())
            .add_media(vec![NumericId::from(media_id)], Vec::<NumericId>::new());
    
        tweet.send().await?;
        info!("Tweet posted successfully");
        Ok(())
    }

    fn name(&self) -> &'static str {
        "Twitter"
    }
}

impl TwitterService {
    /**
     * Uploads an image to Twitter from a file
     */
    async fn upload_image_from_file(&self, file_path: &str) -> Result<u64, NorppaliveError> {
        let token_secrets = TokenSecrets::new(&CONFIG.twitter.consumer_key, &CONFIG.twitter.consumer_secret, &CONFIG.twitter.token, &CONFIG.twitter.token_secret);
        let reqwest_client = reqwest_old::Client::new();
        let res = upload_image_from_file(
            &token_secrets,
            reqwest_client,
            MediaCategory::TweetImage,
            &PathBuf::from(file_path)
        ).await?;
    
        match res {
            EndpointRet::Ok(res) => Ok(res.media_id),
            EndpointRet::Other(err) => {
                error!("Error uploading image to Twitter: {:?}", err);
                Err(NorppaliveError::Other(format!("Error uploading image to Twitter: {:?}", err)))
            },
            _ => {
                error!("Error uploading image to Twitter: Unknown error");
                Err(NorppaliveError::Other(format!("Unknown Twitter error: {:?}", res)))
            }
        }
    }
}

