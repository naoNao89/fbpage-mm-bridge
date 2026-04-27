use anyhow::{Context, Result};
use facebook_graph_service::services::MattermostDbClient;
use sqlx::{postgres::PgPoolOptions, PgPool};

/// Release-10 schema compatibility tests for SQL touched by `MattermostDbClient`.
///
/// CI supplies `MATTERMOST_SCHEMA_TEST_DATABASE_URL` against an ephemeral
/// Mattermost Postgres database. The test is ignored locally by default because
/// it mutates the target database.
#[tokio::test]
#[ignore]
async fn mattermost_db_bypass_writes_match_release_10_schema() -> Result<()> {
    let database_url = std::env::var("MATTERMOST_SCHEMA_TEST_DATABASE_URL")
        .context("MATTERMOST_SCHEMA_TEST_DATABASE_URL must point at an ephemeral Mattermost DB")?;
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&database_url)
        .await?;
    let client = MattermostDbClient::new(&database_url, 2).await?;

    soft_delete_flips_only_active_posts(&pool, &client).await?;
    archive_unarchive_handles_active_archived_and_missing_channels(&pool, &client).await?;
    send_bot_dm_creates_members_post_and_reuses_sorted_channel(&pool, &client).await?;

    Ok(())
}

async fn soft_delete_flips_only_active_posts(
    pool: &PgPool,
    client: &MattermostDbClient,
) -> Result<()> {
    let channel_id = seed_channel(pool, "O", 0).await?;
    let active_1 = seed_post(pool, &channel_id, "user-a", "active-1", 0).await?;
    let active_2 = seed_post(pool, &channel_id, "user-a", "active-2", 0).await?;
    let deleted = seed_post(pool, &channel_id, "user-a", "already-deleted", 123).await?;

    let affected = client.soft_delete_all_posts_in_channel(&channel_id).await?;
    assert_eq!(affected, 2);

    let rows: Vec<(String, i64)> =
        sqlx::query_as("SELECT id, deleteat FROM posts WHERE channelid = $1 ORDER BY id")
            .bind(&channel_id)
            .fetch_all(pool)
            .await?;
    assert_eq!(rows.len(), 3);
    assert!(rows.iter().find(|(id, _)| id == &active_1).unwrap().1 > 0);
    assert!(rows.iter().find(|(id, _)| id == &active_2).unwrap().1 > 0);
    assert_eq!(rows.iter().find(|(id, _)| id == &deleted).unwrap().1, 123);

    let counters: (i64, i64) = sqlx::query_as(
        "SELECT COALESCE(lastpostat, 0), COALESCE(totalmsgcount, 0)
         FROM channels WHERE id = $1",
    )
    .bind(&channel_id)
    .fetch_one(pool)
    .await?;
    assert_eq!(counters, (0, 0));

    let empty_channel = seed_channel(pool, "O", 0).await?;
    assert_eq!(
        client
            .soft_delete_all_posts_in_channel(&empty_channel)
            .await?,
        0
    );
    assert_eq!(
        client
            .soft_delete_all_posts_in_channel("missing-channel")
            .await?,
        0
    );

    Ok(())
}

async fn archive_unarchive_handles_active_archived_and_missing_channels(
    pool: &PgPool,
    client: &MattermostDbClient,
) -> Result<()> {
    let active = seed_channel(pool, "O", 0).await?;
    client.archive_channel(&active).await?;
    assert!(channel_deleteat(pool, &active).await? > 0);

    client.archive_channel(&active).await?;
    assert!(channel_deleteat(pool, &active).await? > 0);

    client.unarchive_channel(&active).await?;
    assert_eq!(channel_deleteat(pool, &active).await?, 0);

    let missing_archive = client.archive_channel("missing-channel").await.unwrap_err();
    assert!(missing_archive.to_string().contains("not found"));

    let missing_unarchive = client
        .unarchive_channel("missing-channel")
        .await
        .unwrap_err();
    assert!(missing_unarchive
        .to_string()
        .contains("not found or not archived"));

    Ok(())
}

async fn send_bot_dm_creates_members_post_and_reuses_sorted_channel(
    pool: &PgPool,
    client: &MattermostDbClient,
) -> Result<()> {
    let bot = seed_user(pool, "bot").await?;
    let user = seed_user(pool, "target").await?;

    let (post_id, channel_id) = client.send_bot_dm(&bot, &user, "hello").await?;
    let expected_name = sorted_dm_name(&bot, &user);
    let channel: (String, i64, i64) = sqlx::query_as(
        "SELECT name, COALESCE(deleteat, 0), COALESCE(totalmsgcount, 0)
         FROM channels WHERE id = $1",
    )
    .bind(&channel_id)
    .fetch_one(pool)
    .await?;
    assert_eq!(channel.0, expected_name);
    assert_eq!(channel.1, 0);
    assert!(channel.2 >= 1);

    let members: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM channelmembers WHERE channelid = $1")
            .bind(&channel_id)
            .fetch_one(pool)
            .await?;
    assert_eq!(members.0, 2);

    let post: (String, String, String) =
        sqlx::query_as("SELECT channelid, userid, message FROM posts WHERE id = $1")
            .bind(&post_id)
            .fetch_one(pool)
            .await?;
    assert_eq!(post.0, channel_id);
    assert_eq!(post.1, bot);
    assert_eq!(post.2, "hello");

    client.archive_channel(&channel_id).await?;
    let (second_post_id, second_channel_id) = client.send_bot_dm(&user, &bot, "again").await?;
    assert_eq!(second_channel_id, channel_id);
    assert_ne!(second_post_id, post_id);
    assert_eq!(channel_deleteat(pool, &channel_id).await?, 0);

    Ok(())
}

async fn seed_user(pool: &PgPool, label: &str) -> Result<String> {
    let id = new_mattermost_id();
    let now = chrono::Utc::now().timestamp_millis();
    let username = format!("test-{label}-{id}");
    let email = format!("{username}@example.invalid");
    sqlx::query(
        "INSERT INTO users
           (id, createat, updateat, deleteat, username, email, emailverified, roles, props, notifyprops, lastpasswordupdate, lastpictureupdate, failedattempts, locale, mfaactive, lastlogin)
         VALUES
           ($1, $2, $2, 0, $3, $4, true, 'system_user', '{}', '{}', 0, 0, 0, 'en', false, 0)",
    )
    .bind(&id)
    .bind(now)
    .bind(username)
    .bind(email)
    .execute(pool)
    .await?;
    Ok(id)
}

async fn seed_channel(pool: &PgPool, channel_type: &str, deleteat: i64) -> Result<String> {
    let id = new_mattermost_id();
    let now = chrono::Utc::now().timestamp_millis();
    let name = format!("test-{}", id.replace('-', ""));
    sqlx::query(
        "INSERT INTO channels
           (id, createat, updateat, deleteat, teamid, type, displayname, name, header, purpose, lastpostat, totalmsgcount)
         VALUES
           ($1, $2, $2, $3, NULL, $4::channel_type, 'Test Channel', $5, '', '', 0, 0)",
    )
    .bind(&id)
    .bind(now)
    .bind(deleteat)
    .bind(channel_type)
    .bind(name)
    .execute(pool)
    .await?;
    Ok(id)
}

async fn seed_post(
    pool: &PgPool,
    channel_id: &str,
    user_id: &str,
    message: &str,
    deleteat: i64,
) -> Result<String> {
    let id = new_mattermost_id();
    let now = chrono::Utc::now().timestamp_millis();
    sqlx::query(
        "INSERT INTO posts
           (id, createat, updateat, deleteat, userid, channelid, rootid, originalid, message, type, props, hashtags, fileids, hasreactions, editat)
         VALUES
           ($1, $2, $2, $3, $4, $5, '', '', $6, '', '{}', '', '[]', false, 0)",
    )
    .bind(&id)
    .bind(now)
    .bind(deleteat)
    .bind(user_id)
    .bind(channel_id)
    .bind(message)
    .execute(pool)
    .await?;
    Ok(id)
}

async fn channel_deleteat(pool: &PgPool, channel_id: &str) -> Result<i64> {
    let row: (i64,) = sqlx::query_as("SELECT COALESCE(deleteat, 0) FROM channels WHERE id = $1")
        .bind(channel_id)
        .fetch_one(pool)
        .await?;
    Ok(row.0)
}

fn sorted_dm_name(user_1: &str, user_2: &str) -> String {
    let (id1, id2) = if user_1 < user_2 {
        (user_1, user_2)
    } else {
        (user_2, user_1)
    };
    format!("__{id1}__{id2}__")
}

fn new_mattermost_id() -> String {
    uuid::Uuid::new_v4()
        .simple()
        .to_string()
        .chars()
        .take(26)
        .collect()
}
