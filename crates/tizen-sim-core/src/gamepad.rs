//! The `http://samsung.com/tv/metadata/gamepad` launch check. An App naming
//! gamepad models in it is checked, "when the application is launched", for
//! one of them being connected. If none is, the platform shows a popup before
//! the App runs: "You must connect the DUALSHOCK®4 controller to play this
//! game. Do you want to play anyway?", with Play, Cancel and Connect Now (TV
//! guides/user-interaction/gamepad.html, "Checking for Specific Gamepads",
//! and its figure).
//!
//! Unverified, as the guide doesn't say:
//! - How a TV tells a connected gamepad's model from its name in the metadata.
//!   The simulator can't either, so the developer declares which models count
//!   as connected (`SessionConfig::gamepads`).
//! - What Cancel and Connect Now do. Here neither launches the App.
//! - The popup's wording for several models, and how a model name like
//!   `dualshock4` is shown (the figure shows "DUALSHOCK®4"). Here the names are
//!   shown as written in the metadata.
//!
//! Gamepads themselves reach every App through the W3C Gamepad API, which the
//! Engine provides. The guide doesn't tie that to the metadata.

use base64::Engine as _;
use serde_json::Value;

/// Whether launching needs the popup: the App names gamepad models and none
/// of them is among the `connected` ones. Names match ignoring ASCII case.
pub fn needs_popup(required: &[String], connected: &[String]) -> bool {
    !required.is_empty()
        && !required
            .iter()
            .any(|model| connected.iter().any(|c| c.eq_ignore_ascii_case(model)))
}

/// What the developer chose in the popup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Choice {
    Play,
    Cancel,
    ConnectNow,
}

/// The popup as a page for the Engine, naming the required `models`.
pub(crate) fn popup_url(models: &[String]) -> String {
    let models = models
        .iter()
        .map(|model| escape_html(model))
        .collect::<Vec<_>>()
        .join(" or ");
    let page = include_str!("gamepad_popup.html").replace("{{MODELS}}", &models);
    format!(
        "data:text/html;charset=utf-8;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(page)
    )
}

/// Evaluated in the popup page: the developer's choice so far, or `null`.
pub(crate) const CHOICE_EXPRESSION: &str = "window.__tizenSimGamepadChoice || null";

/// Reads the value of `CHOICE_EXPRESSION`.
pub(crate) fn parse_choice(value: &Value) -> Option<Choice> {
    match value.as_str()? {
        "play" => Some(Choice::Play),
        "cancel" => Some(Choice::Cancel),
        "connect" => Some(Choice::ConnectNow),
        _ => None,
    }
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_popup_shows_when_no_required_model_is_connected() {
        assert!(!needs_popup(&[], &[]));
        assert!(needs_popup(&names(&["dualshock4"]), &[]));
        assert!(needs_popup(
            &names(&["dualshock4"]),
            &names(&["usbgamepad"])
        ));
        assert!(!needs_popup(
            &names(&["usbgamepad", "dualshock4"]),
            &names(&["DualShock4"])
        ));
    }
}
