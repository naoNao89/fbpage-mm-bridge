use anyhow::{Context, Result};
use facebook_graph_service::services::MattermostDbClient;

/// Schema-compatibility smoke test for the SQL touched by `MattermostDbClient`.
///
/// CI supplies `MATTERMOST_SCHEMA_TEST_DATABASE_URL` against an ephemeral
/// Mattermost Postgres database. The test is ignored locally by default because
/// it requires a running Mattermost schema.
#[tokio::test]
#[ignore]
async fn mattermost_db_bypass_sql_matches_release_10_schema() -> Result<()> {
    let database_url = std::env::var("MATTERMOST_SCHEMA_TEST_DATABASE_URL")
        .context("MATTERMOST_SCHEMA_TEST_DATABASE_URL must point at a Mattermost release-10 DB")?;
    let client = MattermostDbClient::new(&database_url, 2).await?;

    let version = client.schema_version().await?;
    assert!(
        version
            .as_deref()
            .map(|v| v.starts_with("10."))
            .unwrap_or(false),
        "expected Mattermost release-10 schema, got {version:?}"
    );

    Ok(())
}
