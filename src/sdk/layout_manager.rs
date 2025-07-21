use chrono::Utc;
use serde::{Deserialize, Serialize};
use tracing::warn;

/// Layout types for AR display
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LayoutType {
    TextWall,
    DoubleTextWall,
    ReferenceCard,
    DashboardCard,
    BitmapView,
}

/// View types for display
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ViewType {
    Main,
    Dashboard,
}

/// Base layout trait
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "layoutType")]
pub enum Layout {
    #[serde(rename = "text_wall")]
    TextWall { text: String },
    #[serde(rename = "double_text_wall")]
    DoubleTextWall {
        #[serde(rename = "topText")]
        top_text: String,
        #[serde(rename = "bottomText")]
        bottom_text: String,
    },
    #[serde(rename = "reference_card")]
    ReferenceCard { title: String, text: String },
    #[serde(rename = "dashboard_card")]
    DashboardCard {
        #[serde(rename = "leftText")]
        left_text: String,
        #[serde(rename = "rightText")]
        right_text: String,
    },
    #[serde(rename = "bitmap_view")]
    BitmapView { data: String },
}

/// Display request message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisplayRequest {
    pub r#type: String,
    #[serde(rename = "packageName")]
    pub package_name: String,
    #[serde(rename = "sessionId")]
    pub session_id: String,
    pub view: ViewType,
    pub layout: Layout,
    #[serde(rename = "durationMs")]
    pub duration_ms: Option<u64>,
    pub timestamp: String,
}

/// Layout Manager for controlling AR displays
#[derive(Debug)]
pub struct LayoutManager {
    package_name: String,
    session_id: String,
}

impl LayoutManager {
    pub fn new(package_name: String, session_id: String) -> Self {
        Self {
            package_name,
            session_id,
        }
    }

    /// Show a simple text wall
    pub fn show_text_wall(
        &self,
        text: impl Into<String>,
        view: Option<ViewType>,
        duration_ms: Option<u64>,
    ) -> DisplayRequest {
        let text = text.into();
        if text.len() > 1000 {
            warn!(
                "⚠️ TextWall text is very long ({}), this may cause performance issues",
                text.len()
            );
        }

        DisplayRequest {
            r#type: "display_event".to_string(),
            package_name: self.package_name.clone(),
            session_id: self.session_id.clone(),
            view: view.unwrap_or(ViewType::Main),
            layout: Layout::TextWall { text },
            duration_ms,
            timestamp: Utc::now().to_rfc3339(),
        }
    }

    /// Show a double text wall with top and bottom text
    pub fn show_double_text_wall(
        &self,
        top_text: impl Into<String>,
        bottom_text: impl Into<String>,
        view: Option<ViewType>,
        duration_ms: Option<u64>,
    ) -> DisplayRequest {
        DisplayRequest {
            r#type: "display_event".to_string(),
            package_name: self.package_name.clone(),
            session_id: self.session_id.clone(),
            view: view.unwrap_or(ViewType::Main),
            layout: Layout::DoubleTextWall {
                top_text: top_text.into(),
                bottom_text: bottom_text.into(),
            },
            duration_ms,
            timestamp: Utc::now().to_rfc3339(),
        }
    }
}
