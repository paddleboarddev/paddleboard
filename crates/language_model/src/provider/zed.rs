use crate::{LanguageModelProviderId, LanguageModelProviderName};

pub const PADDLEBOARD_CLOUD_PROVIDER_ID: LanguageModelProviderId = LanguageModelProviderId::new("zed.dev");
// PaddleBoard: this is Zed's hosted LLM service, not a PaddleBoard offering — display it
// as "Zed" rather than rebranding it, since the fork has no unique cloud model service.
pub const PADDLEBOARD_CLOUD_PROVIDER_NAME: LanguageModelProviderName =
    LanguageModelProviderName::new("Zed");
