//! Event Manager for handling WebSocket events and user subscriptions
use dashmap::{DashMap, DashSet};
use std::sync::Arc;
use tracing::error;

use crate::sdk::events::{
    ButtonPressData, EventData, HeadPositionData, StreamType, SystemEvent, TranscriptionData,
};

/// Type alias for event handlers
pub type EventHandler = Box<dyn Fn(&EventData) + Send + Sync>;
pub type SystemEventHandler = Box<dyn Fn(&SystemEvent) + Send + Sync>;

/// Event Manager for handling WebSocket events and user subscriptions
pub struct EventManager {
    pub stream_handlers: Arc<DashMap<StreamType, Vec<EventHandler>>>,
    pub system_handlers: Arc<DashMap<String, Vec<SystemEventHandler>>>,
    pub active_subscriptions: Arc<DashSet<StreamType>>,
}

impl std::fmt::Debug for EventManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventManager")
            .field("active_subscriptions", &self.active_subscriptions)
            .finish()
    }
}

impl EventManager {
    pub fn new() -> Self {
        Self {
            stream_handlers: Arc::new(DashMap::new()),
            system_handlers: Arc::new(DashMap::new()),
            active_subscriptions: Arc::new(DashSet::new()),
        }
    }

    /// Add a handler for a specific stream type
    pub fn on_stream<F>(&self, stream_type: StreamType, handler: F)
    where
        F: Fn(&EventData) + Send + Sync + 'static,
    {
        self.stream_handlers
            .entry(stream_type.clone())
            .or_default()
            .push(Box::new(handler));

        // Add to active subscriptions
        self.active_subscriptions.insert(stream_type);
    }

    /// Add a handler for system events
    pub fn on_system<F>(&self, event_type: &str, handler: F)
    where
        F: Fn(&SystemEvent) + Send + Sync + 'static,
    {
        self.system_handlers
            .entry(event_type.to_string())
            .or_default()
            .push(Box::new(handler));
    }

    /// Convenience method for transcription events
    pub fn on_transcription<F>(&self, handler: F)
    where
        F: Fn(&TranscriptionData) + Send + Sync + 'static,
    {
        self.on_stream(StreamType::Transcription, move |data| {
            if let EventData::Transcription(transcription) = data {
                handler(transcription);
            }
        })
    }

    /// Convenience method for button press events
    pub fn on_button_press<F>(&self, handler: F)
    where
        F: Fn(&ButtonPressData) + Send + Sync + 'static,
    {
        self.on_stream(StreamType::ButtonPress, move |data| {
            if let EventData::ButtonPress(button_press) = data {
                handler(button_press);
            }
        })
    }

    /// Convenience method for head position events
    pub fn on_head_position<F>(&self, handler: F)
    where
        F: Fn(&HeadPositionData) + Send + Sync + 'static,
    {
        self.on_stream(StreamType::HeadPosition, move |data| {
            if let EventData::HeadPosition(head_position) = data {
                handler(head_position);
            }
        })
    }

    /// Emit a stream event to all registered handlers
    pub fn emit_stream_event(&self, stream_type: &StreamType, data: &EventData) {
        if let Some(stream_handlers) = self.stream_handlers.get(stream_type) {
            for handler in stream_handlers.iter() {
                match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    handler(data);
                })) {
                    Ok(()) => {}
                    Err(_) => {
                        error!("🚨 Handler panicked for stream type: {:?}", stream_type);
                    }
                }
            }
        }
    }

    /// Emit a system event to all registered handlers
    pub fn emit_system_event(&self, event_type: &str, event: &SystemEvent) {
        if let Some(system_handlers) = self.system_handlers.get(event_type) {
            for handler in system_handlers.iter() {
                match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    handler(event);
                })) {
                    Ok(()) => {}
                    Err(_) => {
                        error!("🚨 System event handler panicked for event: {}", event_type);
                    }
                }
            }
        }
    }
}
