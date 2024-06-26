use std::path::PathBuf;

use chrono::Utc;
use image::DynamicImage;
use megalodon::{entities::{StatusVisibility, UploadMedia}, megalodon::PostStatusInputOptions};
use rand::prelude::*;
use tracing::{debug, error, info};
use twitter_api_v1::{endpoints::{media::upload_media::upload_image_from_file, EndpointRet}, objects::MediaCategory, TokenSecrets};
use twitter_v2::{authorization::Oauth1aToken, id::NumericId, TwitterApi};
use atrium_xrpc_client::reqwest::ReqwestClientBuilder;

use crate::{config::Service, CONFIG, LAST_IMAGE_SAVE_TIME, LAST_POST_TIME};


// struct KVStoreRequest {
//     value: String,
// }

pub async fn should_post() -> bool {
    let last_post_time = LAST_POST_TIME.load(std::sync::atomic::Ordering::Relaxed);
    return last_post_time + CONFIG.output.post_interval > Utc::now().timestamp();
}

pub async fn should_save_image() -> bool {
    let last_image_save_time = LAST_IMAGE_SAVE_TIME.load(std::sync::atomic::Ordering::Relaxed);
    return last_image_save_time + CONFIG.output.image_save_interval > Utc::now().timestamp();
}

pub async fn post_to_social_media(image: DynamicImage) -> Result<(), String> {
    let services = &CONFIG.output.services;

    info!("Posting to social media services: {:?}", services);

    // Save the image
    let image_path = format!("{}/image-{}.jpg", &CONFIG.output.output_file_folder, Utc::now().timestamp());
    image.save(&image_path).map_err(|err| format!("Error saving image: {}", err))?;

    // Get a random message
    let messages = &CONFIG.output.messages;
    let message = messages.get(rand::thread_rng().gen_range(0..messages.len())).unwrap();

    for service in services {
        match service {
            Service::Twitter => {
                let _ = post_to_twitter(message, &image_path).await?;
            }
            Service::Mastodon => {
                let _ = post_to_mastodon(&message, &image_path).await?;
            }
            Service::Bluesky => {
                let _ = post_to_bluesky(&message, &image_path).await?;
            }
        }
    }

    Ok(())
}

async fn post_to_bluesky(message: &str, image_path: &str) -> Result<(), String> {
    let reqwest_client = reqwest::Client::new();
    let client = ReqwestClientBuilder::new(CONFIG.bluesky.host.clone())
        .client(reqwest_client)
        .build();


    Ok(())
}

async fn post_to_mastodon(message: &str, image_path: &str) -> Result<(), String> {
    info!("Logging in to Mastodon");
    let client = megalodon::generator(
        megalodon::SNS::Mastodon,
        CONFIG.mastodon.host.clone(),
        Some(CONFIG.mastodon.token.clone()),
        None,
    );

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
        visibility: Some(StatusVisibility::Private),
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

async fn post_to_twitter(message: &str, image_path: &str) -> Result<(), String> {
    info!("Posting to Twitter");
    let auth_token = Oauth1aToken::new(&CONFIG.twitter.consumer_key, &CONFIG.twitter.consumer_secret, &CONFIG.twitter.token, &CONFIG.twitter.token_secret);
    info!("Auth token created, uploading the image");
    let media_id = upload_image_to_twitter(image_path).await?;
    info!("Image uploaded to Twitter with media id {}", media_id);

    let mut twitter_api = TwitterApi::new(auth_token).post_tweet();
    let tweet = twitter_api
        .text(message.to_string())
        .add_media(vec![NumericId::from(media_id)], Vec::<NumericId>::new());

    tweet.send().await.map_err(|err| format!("Error posting tweet: {}", err))?;
    info!("Tweet posted successfully");
    Ok(())
}

async fn upload_image_to_twitter(image_path: &str) -> Result<u64, String> {
    let token_secrets = TokenSecrets::new(&CONFIG.twitter.consumer_key, &CONFIG.twitter.consumer_secret, &CONFIG.twitter.token, &CONFIG.twitter.token_secret);
    let reqwest_client = reqwest::Client::new();
    let res = upload_image_from_file(
        &token_secrets,
        reqwest_client,
        MediaCategory::TweetImage,
        &PathBuf::from(image_path)
    ).await
    .map_err(|err| format!("Error uploading image to Twitter: {}", err))?;

    return match res {
        EndpointRet::Ok(res) => Ok(res.media_id),
        EndpointRet::Other(err) => {
            error!("Error uploading image to Twitter: {:?}", err);
            Err(format!("Error uploading image to Twitter: {:?}", err))
        },
        _ => {
            error!("Error uploading image to Twitter: Unknown error");
            Err("Unknown error".into())
        }
    };
}

// async fn read_from_kvstore(key: &str) -> Result<String, String> {
//     let client = reqwest::Client::new();
//     let response = client.get(format!("{}{}", &CONFIG.output.kvstore_url, key).as_str())
//         .bearer_auth(&self.kvstore_token)
//         .json(&body)
//         .send().await?;

//     if response.status() != 200 {
//         return Err("Error setting game to KVStore".into());
//     }

//     Ok(())
// }


// async fn save_to_kvstore(key: &str, value: &str) -> Result<String, String> {
//     let body = KVStoreRequest {
//         value: game.to_string(),
//     };

//     let response = self.http_client.post(format!("{}{}", KVSTORE_URL, KVSTORE_KEY).as_str())
//         .bearer_auth(&self.kvstore_token)
//         .json(&body)
//         .send().await?;

//     if response.status() != 200 {
//         return Err("Error setting game to KVStore".into());
//     }

//     Ok(())
// }
