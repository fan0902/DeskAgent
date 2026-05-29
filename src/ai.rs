use crate::config::AppConfig;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AiMode {
    Explain,
    Rewrite,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiRequest {
    pub mode: AiMode,
    pub instruction: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiResponse {
    pub text: String,
    pub rewritten_content: Option<String>,
}

pub trait AiClient {
    fn request(&self, config: &AppConfig, request: &AiRequest) -> anyhow::Result<AiResponse>;
}

pub struct StubAiClient;

impl AiClient for StubAiClient {
    fn request(&self, _config: &AppConfig, request: &AiRequest) -> anyhow::Result<AiResponse> {
        let text = format!(
            "{}: {}",
            match request.mode {
                AiMode::Explain => "explain",
                AiMode::Rewrite => "rewrite",
            },
            request.instruction
        );
        Ok(AiResponse {
            text,
            rewritten_content: if matches!(request.mode, AiMode::Rewrite) {
                Some(request.content.clone())
            } else {
                None
            },
        })
    }
}
