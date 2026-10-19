//! What the Plugins section saves: the TidaLuna opt-in switch. Plugins
//! themselves (installed, enabled, their own settings) live in the Luna data
//! directory (`docs/_reference/luna-plugins.md`).

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PluginSettings {
    /// Run the Luna compatibility runtime (TidaLuna plugins). Off by default:
    /// nothing starts and no plugin UI shows until the user opts in.
    pub luna_enabled: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn older_settings_files_still_load() {
        let parsed: PluginSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(parsed, PluginSettings::default());
        assert!(!parsed.luna_enabled);
        // Every field the removed native ports used to save is ignored.
        let old = r##"{
            "luna_enabled": true,
            "luna_suppressed": ["VolumeScroll"],
            "volume_scroll": {"enabled": true, "step": 2, "step_shift": 1},
            "discord_rpc": {"enabled": true, "display_on_pause": true, "artist_icon": true,
                "playlist_button": true, "status_text": "{track} by {artist}", "pipe_id": 0},
            "native_fullscreen": {"enabled": true},
            "no_remix": {"enabled": true, "skip_current": true, "notify": true,
                "keep_anniversary_mixes": true, "extra_words": "", "allow_words": "",
                "allowed_ids": [], "remix_ids": ["1"]},
            "total_play_time": {"enabled": true},
            "scroll_to_playing": {"enabled": true, "auto_scroll": true,
                "color": [126, 251, 238], "opacity": 20}
        }"##;
        let parsed: PluginSettings = serde_json::from_str(old).unwrap();
        assert!(parsed.luna_enabled);
        let whole: serde_json::Value =
            serde_json::from_str(&format!(r#"{{"plugins": {old}}}"#)).unwrap();
        assert!(whole["plugins"]["no_remix"].is_object());
    }
}
