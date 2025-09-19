//! The partial port of the AugmentOS/MentraOS Cloud WebSocket connection and session management
//! (TPA = Third-Party App).
use anyhow::{Context, Result, bail};
use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use reqwest::Url;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use std::{
    fmt::Display,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::Mutex;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{debug, error, info, warn};

use crate::{
    sdk::{
        event_manager::EventManager,
        events::{
            AudioChunkData, BatteryData, ButtonPressData, CalendarEventData, EventData,
            HeadPositionData, LocationData, PhoneNotificationData, PhotoTakenData, StreamType,
            SystemEvent, TranscriptionData, TranslationData, VadData, VpsCoordinatesData,
        },
        layout_manager::{DisplayRequest, LayoutManager},
    },
    srs::{UserSettings, WebSocketSender},
};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AppConnectionInit {
    pub r#type: String,
    #[serde(rename = "sessionId")]
    pub session_id: String,
    #[serde(rename = "packageName")]
    pub package_name: String,
    #[serde(rename = "apiKey")]
    pub api_key: String,
    pub timestamp: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AppSubscriptionUpdate {
    pub r#type: String,
    #[serde(rename = "packageName")]
    pub package_name: String,
    pub subscriptions: Vec<String>,
    #[serde(rename = "sessionId")]
    pub session_id: String,
    pub timestamp: String,
}

pub(super) fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

#[derive(Debug, Deserialize, PartialEq, Eq, Clone)]
#[serde(transparent)]
pub struct UserId(pub(crate) String);

impl Display for UserId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // 3 chars from start and 9 from the end
        let len = self.0.len();
        if len > 12 {
            write!(f, "{}...{}", &self.0[..3], &self.0[len - 9..])
        } else {
            write!(f, "{}", self.0)
        }
    }
}

impl From<String> for UserId {
    fn from(s: String) -> Self {
        UserId(s)
    }
}

#[derive(Debug)]
pub struct AppSession {
    pub session_id: String,
    pub user_id: UserId,
    pub package_name: String,
    pub api_key: SecretString,
    pub augmentos_websocket_url: Option<String>,
    pub last_updated: u64, // timestamp
    pub user_settings: Arc<UserSettings>,
    pub connected: bool,
    pub reconnect_attempts: u32,
    pub event_manager: EventManager,
    pub layout_manager: LayoutManager,
    pub websocket_sender: WebSocketSender,
}

impl AppSession {
    pub fn new(
        session_id: String,
        user_id: UserId,
        package_name: String,
        api_key: SecretString,
        augmentos_websocket_url: Option<String>,
    ) -> Self {
        let event_manager = EventManager::new();
        let layout_manager = LayoutManager::new(package_name.clone(), session_id.clone());

        Self {
            session_id,
            user_id,
            package_name,
            api_key,
            augmentos_websocket_url,
            last_updated: now_millis(),
            user_settings: Arc::new(UserSettings::new(20, 75)),
            connected: false,
            reconnect_attempts: 0,
            event_manager,
            layout_manager,
            websocket_sender: None,
        }
    }

    /// Connect to AugmentOS Cloud WebSocket
    pub async fn connect(&mut self) -> Result<()> {
        let ws_url = self
            .augmentos_websocket_url
            .as_ref()
            .context("WebSocket URL not provided")?;

        info!(
            "🔌 [{}] Attempting to connect to: {}",
            self.package_name, ws_url
        );

        // Validate URL format before connecting
        let _parsed_url = Url::parse(ws_url).context("Invalid WebSocket URL")?;

        // Add retry logic for connection
        let mut last_error = String::new();
        for attempt in 1..=3 {
            match connect_async(ws_url).await {
                Ok((ws_stream, response)) => {
                    info!(
                        "✅ [{}] Connected to WebSocket for session {} (attempt {})",
                        self.package_name, self.session_id, attempt
                    );
                    debug!(
                        "🔗 [{}] WebSocket response status: {:?}",
                        self.package_name,
                        response.status()
                    );

                    let (write, mut read) = ws_stream.split();
                    let write = Arc::new(Mutex::new(write));

                    // Store the WebSocket sender for later use (e.g., sending display requests)
                    self.websocket_sender = Some(write.clone());

                    // Send connection initialization - use the correct TPA prefix
                    let init_msg = AppConnectionInit {
                        r#type: "tpa_connection_init".to_string(), // Correct message type from TS enum
                        session_id: self.session_id.clone(),
                        package_name: self.package_name.clone(),
                        api_key: self.api_key.expose_secret().to_string(),
                        timestamp: Utc::now().to_rfc3339(),
                    };

                    let init_json = serde_json::to_string(&init_msg)
                        .context("Failed to serialize init message")?;

                    debug!(
                        "🔍 [{}] Sending connection init message: {}",
                        self.package_name, init_json
                    );

                    // Send the message without holding the lock across await
                    let send_result = {
                        let mut sender = write.lock().await;
                        sender.send(Message::Text(init_json.into())).await
                    };

                    if let Err(e) = send_result {
                        bail!("Failed to send init message: {e}");
                    }

                    debug!(
                        "📤 [{}] Connection init message sent successfully",
                        self.package_name
                    );

                    self.connected = true;
                    self.reconnect_attempts = 0;
                    self.last_updated = now_millis();

                    // Spawn background task to handle messages
                    let session_id = self.session_id.clone();
                    let package_name = self.package_name.clone();
                    // Create shared references to the event manager's internal state
                    let stream_handlers = self.event_manager.stream_handlers.clone();
                    let system_handlers = self.event_manager.system_handlers.clone();
                    let active_subscriptions = self.event_manager.active_subscriptions.clone();

                    tokio::spawn(async move {
                        info!(
                            "🎧 [{}] Starting message handler for session {}",
                            package_name, session_id
                        );

                        // Create EventManager instance with shared state
                        let shared_event_manager = EventManager {
                            stream_handlers,
                            system_handlers,
                            active_subscriptions,
                        };
                        let event_manager_arc = Arc::new(shared_event_manager);

                        while let Some(msg) = read.next().await {
                            match msg {
                                Ok(Message::Text(text)) => {
                                    let text_str = text.to_string();
                                    debug!("📨 [{}] Received message: {}", package_name, text_str);
                                    // Handle incoming messages (connection ack, dataSent display request streams, etc.)
                                    if let Err(e) = Self::handle_websocket_message(
                                        &text_str,
                                        event_manager_arc.clone(),
                                    )
                                    .await
                                    {
                                        warn!(
                                            "⚠️ [{}] Error handling message: {}",
                                            package_name, e
                                        );
                                    }
                                }
                                Ok(Message::Binary(data)) => {
                                    debug!(
                                        "📨 [{}] Received binary data: {} bytes",
                                        package_name,
                                        data.len()
                                    );
                                    // Handle binary data (audio, etc.)
                                }
                                Ok(Message::Close(close_frame)) => {
                                    if let Some(cf) = close_frame {
                                        info!(
                                            "👋 [{}] WebSocket connection closed for session {} - Code: {}, Reason: {}",
                                            package_name, session_id, cf.code, cf.reason
                                        );
                                    } else {
                                        info!(
                                            "👋 [{}] WebSocket connection closed for session {}",
                                            package_name, session_id
                                        );
                                    }
                                    break;
                                }
                                Ok(Message::Ping(payload)) => {
                                    debug!(
                                        "🏓 [{}] Received ping: {} bytes",
                                        package_name,
                                        payload.len()
                                    );
                                    let pong_msg = Message::Pong(payload);
                                    if let Err(e) = write.lock().await.send(pong_msg).await {
                                        error!(
                                            "❌ [{}] Failed to send pong response: {}",
                                            package_name, e
                                        );
                                    }
                                }
                                Ok(Message::Pong(payload)) => {
                                    debug!(
                                        "🏓 [{}] Received pong: {} bytes",
                                        package_name,
                                        payload.len()
                                    );
                                }
                                Ok(Message::Frame(_)) => {
                                    debug!("🔧 [{}] Received frame", package_name);
                                }
                                Err(e) => {
                                    error!("❌ [{}] WebSocket error: {}", package_name, e);
                                    break;
                                }
                            }
                        }
                        info!(
                            "🔌 [{}] WebSocket handler task ended for session {}",
                            package_name, session_id
                        );
                    });

                    return Ok(());
                }
                Err(e) => {
                    last_error = format!("WebSocket connection failed: {e}");
                    warn!(
                        "⚠️ [{}] Connection attempt {} failed: {}",
                        self.package_name, attempt, last_error
                    );

                    if attempt < 3 {
                        let delay = Duration::from_millis(1000 * attempt as u64);
                        info!("⏳ [{}] Retrying in {:?}...", self.package_name, delay);
                        tokio::time::sleep(delay).await;
                    }
                }
            }
        }

        error!("❌ [{}] All connection attempts failed", self.package_name);
        self.connected = false;
        bail!(last_error)
    }

    /// Handle incoming WebSocket messages and emit events
    async fn handle_websocket_message(
        message: &str,
        event_manager: Arc<EventManager>,
    ) -> Result<()> {
        // Parse the JSON message
        let json_value: serde_json::Value =
            serde_json::from_str(message).context("Failed to parse JSON")?;

        // Extract message type
        let msg_type = json_value
            .get("type")
            .and_then(|v| v.as_str())
            .context("Message missing 'type' field")?;

        match msg_type {
            "tpa_connection_ack" | "connection_ack" => {
                info!("✅ Connection acknowledged by AugmentOS Cloud");
                // Emit system event
                event_manager.emit_system_event(
                    "connected",
                    &SystemEvent::Connected(json_value.get("settings").cloned()),
                );

                // Handle connection acknowledgment
                if let Some(settings) = json_value.get("settings") {
                    debug!("⚙️ Received settings: {}", settings);
                }
                if let Some(capabilities) = json_value.get("capabilities") {
                    debug!("🔧 Received capabilities: {}", capabilities);
                }
            }
            "tpa_connection_error" | "connection_error" => {
                let error_msg = json_value
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown connection error");
                warn!("❌ Connection error: {}", error_msg);

                // Emit system event
                event_manager
                    .emit_system_event("error", &SystemEvent::Error(error_msg.to_string()));
            }
            "data_stream" => {
                // Handle data streams (transcription, head position, etc.)
                let stream_type = json_value
                    .get("streamType")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                debug!("📊 Received data stream: {}", stream_type);

                // Extract and log the data payload
                if let Some(data) = json_value.get("data") {
                    match stream_type {
                        "translation" => {
                            if let Ok(translation_data) =
                                serde_json::from_value::<TranslationData>(data.clone())
                            {
                                info!("� Translation: {}", translation_data.text);
                                event_manager.emit_stream_event(
                                    &StreamType::Translation,
                                    &EventData::Translation(translation_data),
                                );
                            }
                        }
                        "head_position" => {
                            let res = serde_json::from_value::<HeadPositionData>(data.clone());
                            match res {
                                Ok(head_position_data) => {
                                    debug!(
                                        "👤 Head position: {}, Type: {}",
                                        head_position_data.position, head_position_data.data_type
                                    );
                                    event_manager.emit_stream_event(
                                        &StreamType::HeadPosition,
                                        &EventData::HeadPosition(head_position_data),
                                    );
                                }
                                Err(e) => {
                                    warn!("⚠️ Failed to parse head position data: {e}");
                                }
                            }
                        }
                        "button_press" => {
                            if let Ok(button_press_data) =
                                serde_json::from_value::<ButtonPressData>(data.clone())
                            {
                                info!("🔘 Button press: {}", button_press_data.button_id);
                                event_manager.emit_stream_event(
                                    &StreamType::ButtonPress,
                                    &EventData::ButtonPress(button_press_data),
                                );
                            }
                        }
                        "location_update" => {
                            if let Ok(location_data) =
                                serde_json::from_value::<LocationData>(data.clone())
                            {
                                debug!(
                                    "📍 Location: {}, {}",
                                    location_data.latitude, location_data.longitude
                                );
                                event_manager.emit_stream_event(
                                    &StreamType::LocationUpdate,
                                    &EventData::LocationUpdate(location_data),
                                );
                            }
                        }
                        "vad" => {
                            if let Ok(vad_data) = serde_json::from_value::<VadData>(data.clone()) {
                                debug!("🎙️ Voice activity: {}", vad_data.voice_detected);
                                event_manager.emit_stream_event(
                                    &StreamType::Vad,
                                    &EventData::VoiceActivity(vad_data),
                                );
                            }
                        }
                        "phone_notification" => {
                            if let Ok(notification_data) =
                                serde_json::from_value::<PhoneNotificationData>(data.clone())
                            {
                                info!(
                                    "📱 Notification: {} - {}",
                                    notification_data.title, notification_data.message
                                );
                                event_manager.emit_stream_event(
                                    &StreamType::PhoneNotification,
                                    &EventData::PhoneNotification(notification_data),
                                );
                            }
                        }
                        "calendar_event" => {
                            if let Ok(calendar_data) =
                                serde_json::from_value::<CalendarEventData>(data.clone())
                            {
                                info!("📅 Calendar: {}", calendar_data.title);
                                event_manager.emit_stream_event(
                                    &StreamType::CalendarEvent,
                                    &EventData::CalendarEvent(calendar_data),
                                );
                            }
                        }
                        "glasses_battery_update" => {
                            if let Ok(battery_data) =
                                serde_json::from_value::<BatteryData>(data.clone())
                            {
                                debug!("🔋 Glasses battery: {}%", battery_data.level);
                                event_manager.emit_stream_event(
                                    &StreamType::GlassesBatteryUpdate,
                                    &EventData::GlassesBattery(battery_data),
                                );
                            }
                        }
                        "phone_battery_update" => {
                            if let Ok(battery_data) =
                                serde_json::from_value::<BatteryData>(data.clone())
                            {
                                debug!("📱🔋 Phone battery: {}%", battery_data.level);
                                event_manager.emit_stream_event(
                                    &StreamType::PhoneBatteryUpdate,
                                    &EventData::PhoneBattery(battery_data),
                                );
                            }
                        }
                        "vps_coordinates" => {
                            if let Ok(vps_data) =
                                serde_json::from_value::<VpsCoordinatesData>(data.clone())
                            {
                                debug!(
                                    "🗺️ VPS coordinates: {}, {}, {}",
                                    vps_data.x, vps_data.y, vps_data.z
                                );
                                event_manager.emit_stream_event(
                                    &StreamType::VpsCoordinates,
                                    &EventData::VpsCoordinates(vps_data),
                                );
                            }
                        }
                        "photo_taken" => {
                            if let Ok(photo_data) =
                                serde_json::from_value::<PhotoTakenData>(data.clone())
                            {
                                info!("� Photo taken: {}", photo_data.photo_id);
                                event_manager.emit_stream_event(
                                    &StreamType::PhotoTaken,
                                    &EventData::PhotoTaken(photo_data),
                                );
                            }
                        }
                        "audio_chunk" => {
                            if let Ok(audio_data) =
                                serde_json::from_value::<AudioChunkData>(data.clone())
                            {
                                debug!("🔊 Audio chunk received");
                                event_manager.emit_stream_event(
                                    &StreamType::AudioChunk,
                                    &EventData::AudioChunk(audio_data),
                                );
                            }
                        }
                        _ => {
                            if stream_type.starts_with("transcription") {
                                if let Ok(transcription_data) =
                                    serde_json::from_value::<TranscriptionData>(data.clone())
                                {
                                    let text_len = transcription_data.text.len();
                                    let trim_len = if text_len > 10 { 10 } else { text_len };
                                    info!(
                                        "🎤 Transcription: {}...",
                                        &transcription_data.text[..trim_len]
                                    );
                                    event_manager.emit_stream_event(
                                        &StreamType::Transcription,
                                        &EventData::Transcription(transcription_data),
                                    );
                                }
                            } else {
                                warn!("📊 Unknown stream data: {}", data);
                                event_manager.emit_stream_event(
                                    &StreamType::All,
                                    &EventData::Generic(data.clone()),
                                );
                            }
                        }
                    }
                }
            }
            "settings_update" => {
                info!("⚙️ Settings update received");
                if let Some(settings) = json_value.get("settings") {
                    debug!("⚙️ New settings: {}", settings);
                    event_manager.emit_system_event(
                        "settings_update",
                        &SystemEvent::SettingsUpdate(settings.clone()),
                    );
                }
            }
            "permission_error" => {
                let error_msg = json_value
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Permission denied");
                warn!("🚫 Permission error: {}", error_msg);

                // Extract details if available
                let details = json_value
                    .get("details")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();

                event_manager.emit_system_event(
                    "permission_error",
                    &SystemEvent::PermissionError {
                        message: error_msg.to_string(),
                        details,
                    },
                );
            }
            "dashboard_mode_changed" => {
                if let Some(mode) = json_value.get("mode").and_then(|v| v.as_str()) {
                    info!("🎛️ Dashboard mode changed: {}", mode);
                    event_manager.emit_system_event(
                        "dashboard_mode_change",
                        &SystemEvent::DashboardModeChange {
                            mode: mode.to_string(),
                        },
                    );
                }
            }
            "dashboard_always_on_changed" => {
                if let Some(enabled) = json_value.get("enabled").and_then(|v| v.as_bool()) {
                    info!("🎛️ Dashboard always-on changed: {}", enabled);
                    event_manager.emit_system_event(
                        "dashboard_always_on_change",
                        &SystemEvent::DashboardAlwaysOnChange { enabled },
                    );
                }
            }
            "custom_message" => {
                if let (Some(action), Some(payload)) = (
                    json_value.get("action").and_then(|v| v.as_str()),
                    json_value.get("payload"),
                ) {
                    info!("📨 Custom message: {}", action);
                    event_manager.emit_system_event(
                        "custom_message",
                        &SystemEvent::CustomMessage {
                            action: action.to_string(),
                            payload: payload.clone(),
                        },
                    );
                }
            }
            "app_stopped" => {
                info!("🛑 App stopped notification received");
                // Emit system event for app stopped
                event_manager.emit_system_event(
                    "app_stopped",
                    &SystemEvent::CustomMessage {
                        action: "app_stopped".to_string(),
                        payload: json_value.clone(),
                    },
                );
            }
            "subscription_ack" | "subscription_update_ack" => {
                info!("✅ Subscription acknowledgment received");
                if let Some(subscriptions) =
                    json_value.get("subscriptions").and_then(|v| v.as_array())
                {
                    let subscription_list: Vec<String> = subscriptions
                        .iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect();
                    info!("📡 Active subscriptions: {:?}", subscription_list);
                }
                // Emit system event for subscription acknowledgment
                event_manager.emit_system_event(
                    "subscription_ack",
                    &SystemEvent::CustomMessage {
                        action: "subscription_ack".to_string(),
                        payload: json_value.clone(),
                    },
                );
            }
            _ => {
                debug!(
                    "🤔 Unhandled message type: {} - Message: {}",
                    msg_type, message
                );
            }
        }

        Ok(())
    }

    /// Disconnect from WebSocket
    pub fn disconnect(&mut self) {
        if self.connected {
            info!(
                "👋 [{}] Disconnecting session {}",
                self.package_name, self.session_id
            );
            self.connected = false;
            self.last_updated = now_millis();
        }
    }

    /// Subscribe to event streams
    pub async fn subscribe_to_streams(&self, streams: Vec<String>) -> Result<()> {
        if !self.connected {
            bail!("Session not connected");
        }

        let subscription_msg = AppSubscriptionUpdate {
            r#type: "subscription_update".to_string(),
            package_name: self.package_name.clone(),
            subscriptions: streams.clone(),
            session_id: self.session_id.clone(),
            timestamp: Utc::now().to_rfc3339(),
        };

        // Send the subscription update via WebSocket
        let subscription_json = serde_json::to_string(&subscription_msg)
            .context("Failed to serialize subscription message")?;

        if let Some(sender) = &self.websocket_sender {
            let mut ws_sender = sender.lock().await;
            if let Err(e) = ws_sender
                .send(Message::Text(subscription_json.into()))
                .await
            {
                bail!("Failed to send subscription update: {e}");
            }
            info!(
                "📡 [{}] Sent subscription update for streams: {:?}",
                self.package_name, streams
            );
            Ok(())
        } else {
            bail!("WebSocket sender not available");
        }
    }

    /// Send a display request to AugmentOS Cloud
    pub async fn send_display_request(&self, display_request: &DisplayRequest) -> Result<()> {
        if !self.connected {
            bail!("Session not connected");
        }

        let display_json = serde_json::to_string(display_request)
            .context("Failed to serialize display request")?;
        debug!(
            "📺 [{}] Sending display request: {}",
            self.package_name, display_json
        );
        if let Some(sender) = &self.websocket_sender {
            let mut ws_sender = sender.lock().await;
            if let Err(e) = ws_sender.send(Message::Text(display_json.into())).await {
                bail!("Failed to send display request: {e}");
            }
            debug!("📺 [{}] Sent display request", self.package_name);
            Ok(())
        } else {
            bail!("WebSocket sender not available");
        }
    }

    /// Get a reference to the event manager for setting up event handlers
    pub fn events(&self) -> &EventManager {
        &self.event_manager
    }

    /// Send a text wall display
    pub async fn show_text(&self, text: impl Into<String>, duration_ms: Option<u64>) -> Result<()> {
        let display_request = self.layout_manager.show_text_wall(text, None, duration_ms);
        self.send_display_request(&display_request).await
    }
}
