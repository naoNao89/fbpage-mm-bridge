use anyhow::{Context, Result};
use http::Method;
use minio::s3::builders::ObjectContent;
use minio::s3::creds::StaticProvider;
use minio::s3::http::BaseUrl;
use minio::s3::response::GetPresignedObjectUrlResponse;
use minio::s3::types::S3Api;
use minio::s3::client::ClientBuilder;
use std::time::Duration;

#[derive(Clone)]
pub struct MinioStorage {
    client: minio::s3::Client,
    bucket: String,
    presigned_ttl_secs: u32,
}

impl MinioStorage {
    pub async fn new(
        endpoint: &str,
        access_key: &str,
        secret_key: &str,
        bucket: &str,
        presigned_ttl: Duration,
    ) -> Result<Self> {
        let base_url = endpoint
            .parse::<BaseUrl>()
            .context("invalid MinIO endpoint URL")?;
        let provider = StaticProvider::new(access_key, secret_key, None);

        let client = ClientBuilder::new(base_url)
            .provider(Some(Box::new(provider)))
            .build()
            .map_err(|e| anyhow::anyhow!("failed to build MinIO client: {e}"))?;

        let exists_resp = client
            .bucket_exists(bucket)
            .send()
            .await
            .context("bucket_exists call failed")?;

        if !exists_resp.exists {
            client
                .create_bucket(bucket)
                .send()
                .await
                .context("bucket creation failed")?;
            tracing::info!(bucket, "created MinIO bucket");
        }

        Ok(Self {
            client,
            bucket: bucket.to_string(),
            presigned_ttl_secs: presigned_ttl.as_secs() as u32,
        })
    }

    pub async fn upload_media(
        &self,
        key: &str,
        data: bytes::Bytes,
        content_type: &str,
    ) -> Result<String> {
        let content = ObjectContent::from(data);

        let resp = self
            .client
            .put_object_content(&self.bucket, key, content)
            .content_type(content_type.to_string())
            .send()
            .await
            .context("MinIO upload failed")?;

        let etag = resp.etag;
        tracing::info!(key, %content_type, %etag, "uploaded to MinIO");
        Ok(etag)
    }

    pub async fn presigned_get(&self, key: &str) -> Result<String> {
        let resp: GetPresignedObjectUrlResponse = self
            .client
            .get_presigned_object_url(&self.bucket, key, Method::GET)
            .expiry_seconds(self.presigned_ttl_secs)
            .send()
            .await
            .context("presigned URL generation failed")?;

        Ok(resp.url)
    }

    pub async fn media_markdown(&self, key: &str, media_type: &str, alt_text: &str) -> Result<String> {
        let url = self.presigned_get(key).await?;
        Ok(match media_type {
            "image" => format!("![{alt_text}]({url})"),
            "video" => format!("[▶ {alt_text}]({url})"),
            _ => format!("[{alt_text}]({url})"),
        })
    }

    pub async fn object_exists(&self, key: &str) -> Result<bool> {
        let resp = self
            .client
            .stat_object(&self.bucket, key)
            .send()
            .await;

        match resp {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }
}

pub fn build_media_key(media_type: &str, page_id: &str, message_id: &str, filename: &str) -> String {
    format!("{}/{}/{}/{}", media_type, page_id, message_id, filename)
}

pub fn extract_cdn_expiry(url: &str) -> Option<i64> {
    url.split("oe=")
        .nth(1)?
        .split('&')
        .next()
        .and_then(|hex| i64::from_str_radix(hex, 16).ok())
}

pub const MEDIA_SIZE_SKIP_MINIO: i64 = 50 * 1024 * 1024;
pub const BROWSER_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/133.0.0.0 Safari/537.36";