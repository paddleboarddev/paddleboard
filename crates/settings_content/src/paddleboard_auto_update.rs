// PaddleBoard: settings schema for in-app updates. Lives in settings_content so
// the field deserializes like any other Zed setting; the typed read happens in
// the `auto_update` crate, which already implements `Settings` for the upstream
// `auto_update` toggle and reads this alongside it.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use settings_macros::{MergeFrom, with_fallible_options};

#[with_fallible_options]
#[derive(Debug, Default, PartialEq, Clone, Serialize, Deserialize, JsonSchema, MergeFrom)]
pub struct PaddleboardAutoUpdateContent {
    /// Whether in-app updates may install prerelease builds.
    ///
    /// PaddleBoard's pipeline publishes every release as a prerelease and
    /// promotes it afterwards, and the promotion is a manual step that has been
    /// missed before — releases have sat flagged as prereleases long after they
    /// were the newest build available. Leave this off to follow promoted
    /// releases only; turn it on to ride beta builds as they are cut.
    ///
    /// Default: false
    pub include_prereleases: Option<bool>,
}
