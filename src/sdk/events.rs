use serde::{Deserialize, Serialize};

/// Event types that can be emitted by the event manager
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum StreamType {
    ButtonPress,
    HeadPosition,
    PhoneNotification,
    Transcription,
    Translation,
    GlassesBatteryUpdate,
    PhoneBatteryUpdate,
    GlassesConnectionState,
    LocationUpdate,
    CalendarEvent,
    Vad,
    NotificationDismissed,
    AudioChunk,
    Video,
    RtmpStreamStatus,
    VpsCoordinates,
    PhotoTaken,
    OpenDashboard,
    StartApp,
    StopApp,
    All,
    Wildcard,
}

/// System events not tied to data streams
#[derive(Debug, Clone)]
pub enum SystemEvent {
    Connected(Option<serde_json::Value>), // App settings
    Disconnected(String),
    Error(String),
    SettingsUpdate(serde_json::Value),
    DashboardModeChange {
        mode: String,
    },
    DashboardAlwaysOnChange {
        enabled: bool,
    },
    CustomMessage {
        action: String,
        payload: serde_json::Value,
    },
    PermissionError {
        message: String,
        details: Vec<String>,
    },
    PermissionDenied {
        stream: String,
        required_permission: String,
        message: String,
    },
}

/// Event data for different stream types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum EventData {
    ButtonPress(ButtonPressData),
    HeadPosition(HeadPositionData),
    Transcription(TranscriptionData),
    Translation(TranslationData),
    PhoneNotification(PhoneNotificationData),
    GlassesBattery(BatteryData),
    PhoneBattery(BatteryData),
    LocationUpdate(LocationData),
    CalendarEvent(CalendarEventData),
    VoiceActivity(VadData),
    AudioChunk(AudioChunkData),
    VpsCoordinates(VpsCoordinatesData),
    PhotoTaken(PhotoTakenData),
    Generic(serde_json::Value),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ButtonPressData {
    #[serde(rename = "buttonId")]
    pub button_id: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeadPositionData {
    pub position: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionData {
    pub text: String,
    #[serde(rename = "isFinal")]
    pub is_final: bool,
    #[serde(rename = "startTime")]
    pub start_time: u64,
    #[serde(rename = "endTime")]
    pub end_time: u64,
    #[serde(rename = "transcribeLanguage")]
    pub transcribe_language: Option<String>,
    #[serde(rename = "speakerId")]
    pub speaker_id: Option<String>,
    pub duration: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslationData {
    pub text: String,
    #[serde(rename = "originalText")]
    pub original_text: Option<String>,
    #[serde(rename = "isFinal")]
    pub is_final: bool,
    #[serde(rename = "startTime")]
    pub start_time: u64,
    #[serde(rename = "endTime")]
    pub end_time: u64,
    #[serde(rename = "transcribeLanguage")]
    pub transcribe_language: Option<String>,
    #[serde(rename = "translateLanguage")]
    pub translate_language: Option<String>,
    #[serde(rename = "didTranslate")]
    pub did_translate: Option<bool>,
    #[serde(rename = "speakerId")]
    pub speaker_id: Option<String>,
    pub duration: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhoneNotificationData {
    pub title: String,
    pub message: String,
    pub app: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatteryData {
    pub level: u8,
    #[serde(rename = "isCharging")]
    pub is_charging: bool,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocationData {
    pub latitude: f64,
    pub longitude: f64,
    pub accuracy: Option<f64>,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalendarEventData {
    pub title: String,
    pub start_time: String,
    pub end_time: String,
    pub location: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VadData {
    #[serde(rename = "voiceDetected")]
    pub voice_detected: bool,
    pub confidence: Option<f32>,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioChunkData {
    pub sample_rate: Option<u32>,
    pub duration: Option<u64>,
    pub timestamp: String,
    // Note: actual audio data would be handled separately
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VpsCoordinatesData {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub confidence: Option<f32>,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhotoTakenData {
    #[serde(rename = "photoId")]
    pub photo_id: String,
    pub timestamp: String,
    pub size: Option<u64>,
}
