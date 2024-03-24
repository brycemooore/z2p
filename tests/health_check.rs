use sqlx::{Connection, Executor, PgConnection, PgPool};
use uuid::Uuid;
use zero2prod::{configuration::{get_configuration, DatabaseSettings}, telemetry::{get_subscriber, init_subscriber}};
use once_cell::sync::Lazy;

static TRACING: Lazy<()> = Lazy::new(|| {
    let default_filter_level = "info".to_string();
    let subscriber_name = "test".to_string();
    if std::env::var("TEST_LOG").is_ok() {
        let subscriber = get_subscriber(subscriber_name, default_filter_level, std::io::stdout);
        init_subscriber(subscriber);
    } else {
        let subscriber = get_subscriber(subscriber_name, default_filter_level, std::io::sink);
        init_subscriber(subscriber);
    }
});

pub struct TestApp {
    pub address: String,
    pub db_pool: PgPool,
}
#[tokio::test]
async fn health_check_works() {
    //Arrange
    let app = spawn_app().await;
    let address = format!("{}/health_check", app.address);
    //bring in reqwest
    let client = reqwest::Client::new();

    //Act
    let response = client
        .get(&address)
        .send()
        .await
        .expect("Failed to execute health check works request");

    //Assert
    assert!(response.status().is_success());
    assert_eq!(Some(0), response.content_length());
}

#[tokio::test]
async fn subscribe_returns_200_for_valid_data() {
    //Arrange
    let app = spawn_app().await;
    let address = format!("{}/subscriptions", app.address);

    let client = reqwest::Client::new();
    let body = "name=bryce%20moore&email=brycemooore%40gmail.com";

    //Act
    let response = client
        .post(&address)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await
        .expect("Failed to send post request to /subscriptions");

    let saved = sqlx::query!("SELECT email, name from subscriptions")
        .fetch_one(&app.db_pool)
        .await
        .expect("Failed to fetch saved subscription");

    //Assert
    assert_eq!(200, response.status().as_u16());
    assert_eq!("bryce moore", saved.name);
    assert_eq!("brycemooore@gmail.com", saved.email);
}

#[tokio::test]
async fn subscribe_returns_400_for_invalid_data() {
    //Arrange
    let app = spawn_app().await;
    let address = format!("{}/subscriptions", app.address);
    let client = reqwest::Client::new();
    let test_cases = vec![
        ("name=bryce%20moore", "no email"),
        ("email=brycemooore%40gmail.com", "no name"),
        ("", "no name or email"),
    ];

    for (invalid_body, error_message) in test_cases {
        //Act
        let response = client
            .post(&address)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(invalid_body)
            .send()
            .await
            .expect("Failed to send post request to /subscriptions");

        //Assert
        assert_eq!(
            400,
            response.status().as_u16(),
            "API did not fail with 400 bad request when payload had {}.",
            error_message
        )
    }
}
async fn spawn_app() -> TestApp {
    Lazy::force(&TRACING);

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("Failed to bind random port");
    let port = listener.local_addr().unwrap().port();
    let address = format!("http://127.0.0.1:{}", port);

    let mut configuration = get_configuration().expect("Failed to read configuration");
    configuration.database.database_name = Uuid::new_v4().to_string();
    let connection_pool = configure_database(&configuration.database).await;

    let server = zero2prod::startup::run(listener, connection_pool.clone()).expect("Failed to bind address");
    let _ = tokio::spawn(server);
    TestApp {
        address,
        db_pool: connection_pool,
    }
}

pub async fn configure_database(config: &DatabaseSettings) -> PgPool {
    //create database
    let mut connection = PgConnection::connect(&config.connection_string_without_db())
        .await
        .expect("Failed to connect to database");
    connection
        .execute(format!(r#"CREATE DATABASE "{}";"#, config.database_name).as_str())
        .await
        .expect("Failed to create database");
    let connection_pool = PgPool::connect(&config.connection_string())
        .await
        .expect("Failed to connect to database");
    sqlx::migrate!("./migrations")
        .run(&connection_pool)
        .await
        .expect("Failed to migrate database");

    connection_pool
}
