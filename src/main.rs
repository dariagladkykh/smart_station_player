use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    extract::Json,
    extract::State,
    http::StatusCode,
    response::Html,
    response::Response,
    routing::{get, post},
    Router,
};
use chrono::{DateTime, Utc};
use futures_util::{SinkExt, StreamExt, TryStreamExt};
use mongodb::{
    bson::{doc, Document},
    options::ClientOptions,
    Client, Database,
};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, net::SocketAddr, sync::Arc};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{mpsc, Mutex};
use tower_http::services::ServeDir;

type ActiveClients = Arc<Mutex<HashMap<String, mpsc::Sender<String>>>>;

#[derive(Debug, Deserialize)]
pub struct CommandMessage {
    pub r#type: String,
    pub id: Option<String>,
    pub payload: CommandPayload,
}

#[derive(Debug, Deserialize)]
pub struct CommandPayload {
    pub command: Option<Command>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Command {
    Up,
    Down,
    Left,
    Right,
    Press,
}

#[tokio::main]
async fn main() {
    let mongo_client = create_mongo_client().await.unwrap();
    let database = mongo_client.database("pico_project");
    let state = DatabaseState {
        db: database.clone(),
    };

    let active_clients = Arc::new(Mutex::new(HashMap::new()));
    let active_clients_clone = active_clients.clone();
    let active_clients_for_tcp = active_clients.clone();

    let app = Router::new()
        .route("/", get(root))
        .route("/ws", get(ws_handler))
        .route("/api/hello", post(hello))
        .route("/api/clock", post(insert_clock_data))
        .route("/api/clock", get(get_clock_data))
        .route("/api/sensor", post(insert_sensor_reading))
        .route("/api/sensor", get(get_sensor_readings))
        .nest_service("/assets", ServeDir::new("assets"))
        .with_state(AppState {
            db: state.db,
            clients: active_clients_clone,
        });

    tokio::join!(
        start_http_server(app),
        start_tcp_listener(active_clients_for_tcp)
    );
}

async fn start_http_server(app: Router) {
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn start_tcp_listener(active_clients: ActiveClients) {
    let listener = tokio::net::TcpListener::bind("192.168.4.8:6000")
        .await
        .unwrap();
    println!("TCP listening on port 6000");

    loop {
        let (mut socket, addr) = listener.accept().await.unwrap();
        println!("New TCP connection from {addr}");
        let clients = active_clients.clone();

        tokio::spawn(async move {
            let mut buf = vec![0u8; 1024];

            loop {
                let n = match socket.read(&mut buf).await {
                    Ok(0) => {
                        println!("Connection closed by client");
                        break;
                    }
                    Ok(n) => n,
                    Err(e) => {
                        eprintln!("read error: {:?}", e);
                        break;
                    }
                };

                let msg = &buf[..n];
                if let Ok(json_str) = std::str::from_utf8(msg) {
                    println!("Received TCP msg: {}", json_str);
                    if let Ok(command) = serde_json::from_str::<CommandMessage>(json_str) {
                        println!("Parsed command: {:?}", command);
                        // Handle the command here
                        match command.payload.command {
                            Some(Command::Up) => {
                                println!("Command: Up");
                                // Broadcast to all WebSocket clients
                                let music_cmd = r#"{"type":"music","action":"volume_up"}"#;
                                broadcast_to_clients(&clients, music_cmd).await;
                            }
                            Some(Command::Down) => {
                                println!("Command: Down");
                                let music_cmd = r#"{"type":"music","action":"volume_down"}"#;
                                broadcast_to_clients(&clients, music_cmd).await;
                            }
                            Some(Command::Left) => {
                                println!("Command: Left");
                                let music_cmd = r#"{"type":"music","action":"previous"}"#;
                                broadcast_to_clients(&clients, music_cmd).await;
                            }
                            Some(Command::Right) => {
                                println!("Command: Right");
                                let music_cmd = r#"{"type":"music","action":"next"}"#;
                                broadcast_to_clients(&clients, music_cmd).await;
                            }
                            Some(Command::Press) => {
                                println!("Command: Press");
                                let music_cmd = r#"{"type":"music","action":"play_pause"}"#;
                                broadcast_to_clients(&clients, music_cmd).await;
                            }
                            None => println!("No command found"),
                        }

                        let ack = r#"{"type":"ack","id":"server","payload":{"status":"ok"}}"#;
                        let _ = socket.write_all(ack.as_bytes()).await;
                    }
                } else {
                    eprintln!("Non-UTF8 data received");
                }
            }
        });
    }
}

async fn handle_socket(socket: WebSocket, clients: ActiveClients) {
    let (tx, mut rx) = mpsc::channel(100);
    let (mut sender, mut receiver) = socket.split();

    // Generate a unique client ID
    let client_id = format!("client_{}", rand::random::<u64>());

    // Store the sender for this client
    clients.lock().await.insert(client_id.clone(), tx);

    // Task to receive messages from the client
    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(Message::Text(text))) = receiver.next().await {
            println!("Received WebSocket message: {}", text);
            // Handle client messages if needed
        }
    });

    // Task to send messages to the client
    let mut send_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if sender.send(Message::Text(msg.into())).await.is_err() {
                break;
            }
        }
    });

    // When any task completes, clean up
    tokio::select! {
        _ = &mut recv_task => {
            send_task.abort();
        },
        _ = &mut send_task => {
            recv_task.abort();
        },
    }

    // Remove the client when WebSocket closes
    clients.lock().await.remove(&client_id);
}

async fn root() -> Html<&'static str> {
    Html(include_str!("../assets/index.html"))
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> Response {
    ws.on_upgrade(|socket| handle_socket(socket, state.clients))
}

#[derive(Clone)]
struct DatabaseState {
    db: Database,
}

#[derive(Clone)]
struct AppState {
    db: Database,
    clients: ActiveClients,
}

#[derive(Deserialize)]
struct HelloRequest {
    name: String,
}

#[derive(Serialize)]
struct HelloResponse {
    message: String,
}

#[derive(Serialize, Deserialize)]
struct ClockDataRequest {
    time: String,
    minutes_binary: String,
    seconds_binary: String,
    timestamp: DateTime<Utc>,
}
#[derive(Serialize)]
struct ClockDataResponse {
    time: String,
    minutes_binary: String,
    seconds_binary: String,
    timestamp: DateTime<Utc>,
}

#[derive(Serialize, Deserialize)]
struct SensorReadingRequest {
    value: i32,
    timestamp: DateTime<Utc>,
}

#[derive(Serialize)]
struct SensorReadingResponse {
    value: i32,
    timestamp: DateTime<Utc>,
}

#[derive(Serialize)]
struct InsertResult {
    inserted_id: String,
}

async fn hello(
    State(_state): State<AppState>,
    Json(payload): Json<HelloRequest>,
) -> (StatusCode, Json<HelloResponse>) {
    let response = HelloResponse {
        message: format!("Hello, {}!", payload.name),
    };
    (StatusCode::OK, Json(response))
}

async fn create_mongo_client() -> Result<Client, Box<dyn std::error::Error>> {
    let mut client_options = ClientOptions::parse("mongodb://localhost:27017").await?;
    client_options.app_name = Some("PicoProjectApp".into());
    let client = Client::with_options(client_options)?;
    Ok(client)
}

async fn insert_clock_data(
    State(state): State<AppState>,
    Json(clock): Json<ClockDataRequest>,
) -> (StatusCode, Json<InsertResult>) {
    let collection = state.db.collection::<Document>("clock_data");
    let doc = doc! {
        "time": clock.time,
        "minutes_binary": clock.minutes_binary,
        "seconds_binary": clock.seconds_binary,
        "timestamp": clock.timestamp,
    };
    let result = collection.insert_one(doc).await.unwrap();
    let inserted_id = result.inserted_id.as_object_id().unwrap().to_string();

    (StatusCode::OK, Json(InsertResult { inserted_id }))
}

async fn get_clock_data(
    State(state): State<AppState>,
) -> Result<Json<Vec<ClockDataResponse>>, StatusCode> {
    let collection = state.db.collection::<Document>("clock_data");
    let options = mongodb::options::FindOptions::builder()
        .sort(doc! { "timestamp": -1 })
        .limit(10)
        .build();
    let mut cursor = collection
        .find(doc! {})
        .with_options(options)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut clock_data = Vec::new();
    while let Some(doc) = cursor
        .try_next()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    {
        let time = doc.get_str("time").unwrap().to_string();
        let minutes_binary = doc.get_str("minutes_binary").unwrap().to_string();
        let seconds_binary = doc.get_str("seconds_binary").unwrap().to_string();
        let timestamp = doc
            .get_datetime("timestamp")
            .map(|dt| dt.to_chrono())
            .unwrap_or_else(|_| Utc::now());
        clock_data.push(ClockDataResponse {
            time,
            minutes_binary,
            seconds_binary,
            timestamp,
        });
    }
    Ok(Json(clock_data))
}

async fn insert_sensor_reading(
    State(state): State<AppState>,
    Json(reading): Json<SensorReadingRequest>,
) -> (StatusCode, Json<InsertResult>) {
    let collection = state.db.collection::<Document>("sensor_readings");
    let doc = doc! {
        "value": reading.value,
        "timestamp": reading.timestamp,
    };
    let result = collection.insert_one(doc).await.unwrap();
    let inserted_id = result.inserted_id.as_object_id().unwrap().to_string();

    (StatusCode::OK, Json(InsertResult { inserted_id }))
}

async fn get_sensor_readings(
    State(state): State<AppState>,
) -> Result<Json<Vec<SensorReadingResponse>>, StatusCode> {
    let collection = state.db.collection::<Document>("sensor_readings");
    let options = mongodb::options::FindOptions::builder()
        .sort(doc! { "timestamp": -1 })
        .limit(10)
        .build();
    let mut cursor = collection
        .find(doc! {})
        .with_options(options)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut readings = Vec::new();
    while let Some(doc) = cursor
        .try_next()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    {
        let value = doc.get_i32("value").unwrap();
        let timestamp = doc
            .get_datetime("timestamp")
            .map(|dt| dt.to_chrono())
            .unwrap_or_else(|_| Utc::now());
        readings.push(SensorReadingResponse { value, timestamp });
    }
    Ok(Json(readings))
}

// Helper function to broadcast messages to all WebSocket clients
async fn broadcast_to_clients(clients: &ActiveClients, message: &str) {
    println!("Broadcasting: {}", message);
    let clients_lock = clients.lock().await;
    for (_client_id, tx) in clients_lock.iter() {
        let _ = tx.send(message.to_string()).await;
    }
}