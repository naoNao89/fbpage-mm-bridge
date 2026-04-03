use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::env;
use std::sync::atomic;

pub async fn setup_test_db() -> PgPool {
    dotenvy::dotenv().ok();

    let database_url = env::var("TEST_DATABASE_URL")
        .or_else(|_| env::var("DATABASE_URL"))
        .expect("TEST_DATABASE_URL or DATABASE_URL must be set for tests");

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .expect("Failed to connect to test database");

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("Failed to run migrations");

    pool
}

pub async fn cleanup_test_db(pool: &PgPool) {
    sqlx::query("TRUNCATE TABLE customers CASCADE")
        .execute(pool)
        .await
        .expect("Failed to cleanup test database");
}

static UID_COUNTER: atomic::AtomicU64 = atomic::AtomicU64::new(0);

pub fn unique_platform_user_id() -> String {
    let count = UID_COUNTER.fetch_add(1, atomic::Ordering::SeqCst);
    let tid = std::thread::current().id();
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{:?}:{}:{}", tid, ts, count)
}
