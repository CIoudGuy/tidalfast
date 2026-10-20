//! A plugin's `Settings` React tree, captured by the React shim as plain
//! data the Plugins page draws with native widgets.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A link inside a description (`<LunaLink href>` / `<a href>`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SettingsLink {
    pub text: String,
    pub href: String,
}

/// One option of a select.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SelectOption {
    /// Sent back unchanged in [`SettingValue::Json`].
    pub value: Value,
    pub label: String,
}

/// Fields every titled setting has.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SettingsMeta {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub desc: Option<String>,
    #[serde(default)]
    pub links: Option<Vec<SettingsLink>>,
    #[serde(default)]
    pub disabled: Option<bool>,
    /// A group that was `position: fixed`: drawn over the window, centred.
    #[serde(default)]
    pub overlay: Option<bool>,
    /// A button that was a fixed, window-wide backdrop: drawn as a dimmed
    /// layer over the window that a click on closes.
    #[serde(default)]
    pub scrim: Option<bool>,
    /// The `@luna/ui` or MUI component that made the node
    /// (`LunaNumberSetting`, `LunaSettings`, `Button`): the page draws it in
    /// that component's look. `None` for plain HTML.
    #[serde(default)]
    pub mui: Option<String>,
    /// The component's `variant` prop (MUI's `contained`, `outlined`, `text`).
    #[serde(default)]
    pub variant: Option<String>,
}

/// The input id that renders a crashed settings panel again.
pub const SETTINGS_RETRY: &str = "__retry";

/// A node of a plugin's settings. Interactive nodes carry an `id`; send
/// input for it with [`crate::luna::Luna::settings_input`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum SettingsNode {
    /// `LunaSettings` / `LunaStack` / a titled `LunaSetting`.
    Group {
        #[serde(flatten)]
        meta: SettingsMeta,
        #[serde(default)]
        children: Vec<SettingsNode>,
    },
    /// `LunaSwitchSetting`.
    Switch {
        id: String,
        #[serde(flatten)]
        meta: SettingsMeta,
        checked: bool,
    },
    /// `LunaNumberSetting`.
    Number {
        id: String,
        #[serde(flatten)]
        meta: SettingsMeta,
        value: f64,
        #[serde(default)]
        min: Option<f64>,
        #[serde(default)]
        max: Option<f64>,
        #[serde(default)]
        step: Option<f64>,
        /// Draw it as a slider (`<input type="range">`, MUI `Slider`); needs
        /// `min` and `max`.
        #[serde(default)]
        slider: Option<bool>,
    },
    /// `LunaTextSetting` / `LunaSecureTextSetting` (`secure`: mask it).
    #[serde(rename = "input")]
    Text {
        id: String,
        #[serde(flatten)]
        meta: SettingsMeta,
        value: String,
        #[serde(default)]
        secure: bool,
        #[serde(default)]
        multiline: Option<bool>,
        #[serde(default)]
        placeholder: Option<String>,
    },
    /// `LunaSelectSetting` with `LunaSelectItem` options.
    Select {
        id: String,
        #[serde(flatten)]
        meta: SettingsMeta,
        value: Value,
        options: Vec<SelectOption>,
        #[serde(default)]
        multiple: Option<bool>,
    },
    /// `LunaButtonSetting` / `LunaButton`.
    Button {
        id: String,
        #[serde(flatten)]
        meta: SettingsMeta,
        label: String,
    },
    /// A field that records a key combination: the plugin handles
    /// `keydown` itself (quickExclusiveMode's keybind). Send the combination
    /// as [`SettingValue::Json`] `{key, code, ctrlKey, shiftKey, altKey,
    /// metaKey}`; the plugin gets focus, keydown/keyup and blur events.
    #[serde(rename = "keys")]
    KeyCapture {
        id: String,
        #[serde(flatten)]
        meta: SettingsMeta,
        #[serde(default)]
        value: String,
    },
    /// The plugin's `Settings` component threw (Luna: "Settings crashed");
    /// [`SETTINGS_RETRY`] renders it again.
    Crashed { error: String },
    /// `LunaLink` / `<a>`.
    Link { text: String, href: String },
    /// Plain text (`title`: a heading from `LunaTitle`).
    #[serde(rename = "text")]
    Label {
        text: String,
        #[serde(default)]
        title: Option<bool>,
        #[serde(default)]
        desc: Option<String>,
    },
}

impl SettingsNode {
    /// Parse the shim's JSON. Unknown shapes become an explanatory label
    /// rather than an error, so one odd component never hides the rest.
    pub fn from_json(v: &Value) -> SettingsNode {
        // The shim emits a "text" kind for both labels and text inputs; text
        // inputs always carry an id.
        if v.get("kind").and_then(Value::as_str) == Some("text")
            && v.get("id").is_some()
            && let Ok(n) = serde_json::from_value::<TextInput>(v.clone())
        {
            return SettingsNode::Text {
                id: n.id,
                meta: n.meta,
                value: n.value,
                secure: n.secure,
                multiline: n.multiline,
                placeholder: n.placeholder,
            };
        }
        if v.get("kind").and_then(Value::as_str) == Some("group") {
            let meta: SettingsMeta = serde_json::from_value(v.clone()).unwrap_or_default();
            let children = v
                .get("children")
                .and_then(Value::as_array)
                .map(|a| a.iter().map(SettingsNode::from_json).collect())
                .unwrap_or_default();
            return SettingsNode::Group { meta, children };
        }
        serde_json::from_value(v.clone()).unwrap_or_else(|e| SettingsNode::Label {
            text: format!("(unsupported setting: {e})"),
            title: None,
            desc: None,
        })
    }

    /// Visit every node, depth first.
    pub fn walk<'a>(&'a self, f: &mut dyn FnMut(&'a SettingsNode)) {
        f(self);
        if let SettingsNode::Group { children, .. } = self {
            for c in children {
                c.walk(f);
            }
        }
    }

    /// The node with this id.
    pub fn find(&self, id: &str) -> Option<&SettingsNode> {
        let mut found = None;
        self.walk(&mut |n| {
            if found.is_none() && n.id() == Some(id) {
                found = Some(n);
            }
        });
        found
    }

    pub fn id(&self) -> Option<&str> {
        match self {
            SettingsNode::Switch { id, .. }
            | SettingsNode::Number { id, .. }
            | SettingsNode::Text { id, .. }
            | SettingsNode::Select { id, .. }
            | SettingsNode::Button { id, .. }
            | SettingsNode::KeyCapture { id, .. } => Some(id),
            _ => None,
        }
    }
}

#[derive(Deserialize)]
struct TextInput {
    id: String,
    #[serde(flatten)]
    meta: SettingsMeta,
    #[serde(default)]
    value: String,
    #[serde(default)]
    secure: bool,
    #[serde(default)]
    multiline: Option<bool>,
    #[serde(default)]
    placeholder: Option<String>,
}

/// Input from the Plugins page for one settings node.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SettingValue {
    Bool(bool),
    Number(f64),
    Text(String),
    /// A select option's `value`.
    Json(Value),
    /// A button press.
    Click,
}

impl SettingValue {
    pub(crate) fn to_json(&self) -> Value {
        match self {
            SettingValue::Bool(b) => Value::Bool(*b),
            SettingValue::Number(n) => serde_json::json!(n),
            SettingValue::Text(s) => Value::String(s.clone()),
            SettingValue::Json(v) => v.clone(),
            SettingValue::Click => Value::Null,
        }
    }
}
