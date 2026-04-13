use anyhow::{Context, Result};
use bytes::Bytes;
use reqwest::Client;
use std::time::Duration;

use crate::storage::BROWSER_USER_AGENT;

#[derive(Debug, Clone)]
pub struct AttachmentInfo {
    pub attachment_type: String,
    pub url: String,
    pub mime_type: Option<String>,
    pub name: Option<String>,
    pub size: Option<i64>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub preview_url: Option<String>,
    pub external_id: Option<String>,
}

pub fn build_cdn_client() -> Result<Client> {
    Client::builder()
        .use_rustls_tls()
        .user_agent(BROWSER_USER_AGENT)
        .redirect(reqwest::redirect::Policy::limited(10))
        .timeout(Duration::from_secs(120))
        .connect_timeout(Duration::from_secs(10))
        .build()
        .context("failed to build CDN HTTP client")
}

pub async fn download_from_cdn(client: &Client, url: &str) -> Result<(Bytes, String)> {
    let resp = client
        .get(url)
        .send()
        .await
        .context("CDN download request failed")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("CDN download failed {status}: {body}");
    }

    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();

    let data = resp
        .bytes()
        .await
        .context("failed to read CDN response body")?;

    Ok((data, content_type))
}

pub fn resolve_attachment_type(mime_type: Option<&str>) -> &'static str {
    match mime_type {
        Some(mt) if mt.starts_with("image/") => "image",
        Some(mt) if mt.starts_with("video/") => "video",
        Some(mt) if mt.starts_with("audio/") => "audio",
        _ => "file",
    }
}

pub fn extract_attachments_from_graph(msg: &crate::models::GraphMessage) -> Vec<AttachmentInfo> {
    let mut attachments = Vec::new();

    if let Some(atts) = &msg.attachments {
        for att in &atts.data {
            if let Some(img) = &att.image_data {
                attachments.push(AttachmentInfo {
                    attachment_type: "image".to_string(),
                    url: img.url.clone(),
                    mime_type: att.mime_type.clone(),
                    name: att.name.clone(),
                    size: att.size,
                    width: img.width,
                    height: img.height,
                    preview_url: img.preview_url.clone(),
                    external_id: Some(att.id.clone()),
                });
            }
            if let Some(vid) = &att.video_data {
                attachments.push(AttachmentInfo {
                    attachment_type: "video".to_string(),
                    url: vid.url.clone(),
                    mime_type: att.mime_type.clone(),
                    name: att.name.clone(),
                    size: att.size,
                    width: vid.width,
                    height: vid.height,
                    preview_url: vid.preview_url.clone(),
                    external_id: Some(att.id.clone()),
                });
            }
            if att.image_data.is_none() && att.video_data.is_none() {
                if let Some(file_url) = &att.file_url {
                    let att_type = resolve_attachment_type(att.mime_type.as_deref());
                    attachments.push(AttachmentInfo {
                        attachment_type: att_type.to_string(),
                        url: file_url.clone(),
                        mime_type: att.mime_type.clone(),
                        name: att.name.clone(),
                        size: att.size,
                        width: None,
                        height: None,
                        preview_url: None,
                        external_id: Some(att.id.clone()),
                    });
                }
            }
        }
    }

    attachments
}

pub fn format_attachment_markdown(atts: &[AttachmentInfo]) -> String {
    let mut parts = Vec::new();
    for att in atts {
        match att.attachment_type.as_str() {
            "image" => parts.push(format!(
                "![{}]({})",
                att.name.as_deref().unwrap_or("image"),
                att.url
            )),
            "video" => parts.push(format!(
                "[▶ {}]({})",
                att.name.as_deref().unwrap_or("video"),
                att.url
            )),
            _ => parts.push(format!(
                "[{}]({})",
                att.name.as_deref().unwrap_or("file"),
                att.url
            )),
        }
    }
    parts.join("\n")
}

fn format_cdn_fallback(att: &AttachmentInfo) -> String {
    match att.attachment_type.as_str() {
        "image" => format!(
            "📷 [{}]({})",
            att.name.as_deref().unwrap_or("image"),
            att.url
        ),
        "video" => format!(
            "📹 [{}]({})",
            att.name.as_deref().unwrap_or("video"),
            att.url
        ),
        "audio" => format!(
            "🎵 [{}]({})",
            att.name.as_deref().unwrap_or("audio"),
            att.url
        ),
        _ => format!(
            "📎 [{}]({})",
            att.name.as_deref().unwrap_or("file"),
            att.url
        ),
    }
}

fn detect_content_type(data: &[u8], declared: &str) -> String {
    if data.len() >= 4 {
        let magic = [data[0], data[1], data[2], data[3]];
        if magic[..4] == [0x89, 0x50, 0x4E, 0x47] {
            return "image/png".to_string();
        }
        if magic[..2] == [0xFF, 0xD8] {
            return "image/jpeg".to_string();
        }
        if magic[..4] == [0x47, 0x49, 0x46, 0x38] {
            return "image/gif".to_string();
        }
        if magic[..4] == [0x52, 0x49, 0x46, 0x46] {
            return "image/webp".to_string();
        }
        if data.starts_with(b"%PDF") {
            return "application/pdf".to_string();
        }
        if data.starts_with(b"ftyp") || (data.len() >= 8 && data[4..8] == [b'f', b't', b'y', b'p'])
        {
            return "video/mp4".to_string();
        }
    }
    declared.to_string()
}

fn ensure_extension(filename: &str, content_type: &str) -> String {
    if filename.contains('.') {
        return filename.to_string();
    }
    let ext = match content_type {
        "image/png" => ".png",
        "image/jpeg" => ".jpg",
        "image/gif" => ".gif",
        "image/webp" => ".webp",
        "video/mp4" => ".mp4",
        "video/quicktime" => ".mov",
        "video/x-msvideo" => ".avi",
        "audio/mpeg" => ".mp3",
        "audio/ogg" => ".ogg",
        "application/pdf" => ".pdf",
        _ => "",
    };
    format!("{filename}{ext}")
}

pub async fn process_attachments_for_post(
    state: &crate::AppState,
    mm: &crate::services::MattermostClient,
    channel_id: &str,
    text: &str,
    attachments: &[AttachmentInfo],
    message_id: &str,
    message_db_id: Option<uuid::Uuid>,
    bot_token: Option<&str>,
) -> (String, Vec<String>) {
    if attachments.is_empty() {
        return (text.to_string(), Vec::new());
    }

    let mut file_ids = Vec::new();
    let mut fallback_parts = Vec::new();

    let cdn_client = match build_cdn_client() {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("Failed to build CDN client: {e}");
            let att_markdown = format_attachment_markdown(attachments);
            let combined_text = if text.trim().is_empty() {
                att_markdown
            } else {
                format!("{text}\n{att_markdown}")
            };
            return (combined_text, file_ids);
        }
    };

    for att in attachments {
        let should_download = att
            .size
            .map(|s| s < crate::storage::MEDIA_SIZE_SKIP_MINIO)
            .unwrap_or(true);
        if !should_download {
            tracing::info!(
                "Skipping download of large {} ({:?} bytes)",
                att.attachment_type,
                att.size
            );
            fallback_parts.push(format_cdn_fallback(&att));
            continue;
        }

        let filename = att.name.as_deref().unwrap_or("attachment");
        let mut content_type = att
            .mime_type
            .clone()
            .unwrap_or_else(|| "application/octet-stream".to_string());

        match download_from_cdn(&cdn_client, &att.url).await {
            Ok((data, detected_ct)) => {
                if content_type == "application/octet-stream" {
                    content_type = detected_ct.clone();
                }

                let actual_content_type = detect_content_type(&data, &content_type);
                content_type = actual_content_type.clone();

                let filename_with_ext = ensure_extension(filename, &content_type);

                if let Some(ref minio_storage) = state.minio {
                    let key = crate::storage::build_media_key(
                        &att.attachment_type,
                        &state.config.facebook_page_id,
                        message_id,
                        &filename_with_ext,
                    );
                    if !minio_storage.object_exists(&key).await.unwrap_or(false) {
                        if let Ok(etag) = minio_storage
                            .upload_media(&key, data.clone(), &content_type)
                            .await
                        {
                            let minio_bucket = state.config.minio_bucket.clone();
                            let cdn_expires = crate::storage::extract_cdn_expiry(&att.url);
                            let cdn_expires_at = cdn_expires
                                .map(|ts| chrono::DateTime::from_timestamp(ts, 0))
                                .flatten();

                            if let Some(db_id) = message_db_id {
                                let payload = crate::services::AttachmentPayload {
                                    message_id: db_id,
                                    attachment_type: att.attachment_type.clone(),
                                    external_id: att.external_id.clone(),
                                    name: Some(filename_with_ext.clone()),
                                    mime_type: Some(content_type.clone()),
                                    size_bytes: att.size,
                                    width: att.width,
                                    height: att.height,
                                    cdn_url: Some(att.url.clone()),
                                    cdn_url_expires_at: cdn_expires_at,
                                    minio_key: Some(key.clone()),
                                    minio_bucket: Some(minio_bucket),
                                    minio_etag: Some(etag),
                                    mm_file_id: None,
                                };
                                if let Err(e) = state.message_client.store_attachment(payload).await
                                {
                                    tracing::warn!("Failed to store attachment metadata: {e}");
                                }
                            }
                        }
                    }
                }

                let upload_result = if let Some(bt) = bot_token {
                    mm.upload_file_as_bot(channel_id, data, &filename_with_ext, &content_type, bt)
                        .await
                } else {
                    mm.upload_file(channel_id, data, &filename_with_ext, &content_type)
                        .await
                };

                match upload_result {
                    Ok(file_id) => {
                        tracing::info!(
                            "Uploaded {} to Mattermost: file_id={file_id}",
                            att.attachment_type
                        );
                        file_ids.push(file_id);
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Mattermost file upload failed for {}: {e}, using CDN fallback",
                            att.attachment_type
                        );
                        fallback_parts.push(format_cdn_fallback(&att));
                    }
                }
            }
            Err(e) => {
                tracing::warn!(
                    "CDN download failed for {}: {e}, using CDN fallback",
                    att.attachment_type
                );
                fallback_parts.push(format_cdn_fallback(&att));
            }
        }
    }

    let combined_text = if text.trim().is_empty() && fallback_parts.is_empty() {
        String::new()
    } else if text.trim().is_empty() {
        fallback_parts.join("\n")
    } else if fallback_parts.is_empty() {
        text.to_string()
    } else {
        format!("{}\n{}", text, fallback_parts.join("\n"))
    };

    (combined_text, file_ids)
}
