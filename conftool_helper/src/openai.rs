use async_openai::{
    Client,
    config::OpenAIConfig,
    types::{
        ChatCompletionRequestSystemMessageArgs, CreateChatCompletionRequestArgs, ReasoningEffort,
    },
};
use std::sync::OnceLock;
use tokio::runtime::Runtime;

static CLIENT: OnceLock<Client<OpenAIConfig>> = OnceLock::new();

/// Default model: high performance, low cost
const CHAT_MODEL_DEFAULT: &str = "gpt-5-nano";

/// Prompts directory
pub const SYSTEM_PROMPTS_DIR_PATH: &str = "prompts/system";

/// OpenAI chat response, synchronous wrapper over the async functionality
pub fn chat_response(
    system_prompt: &str,
    user_prompt: &str,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let rt = Runtime::new()?;
    rt.block_on(chat_response_async(system_prompt, user_prompt))
}

/// Gets the OpenAI client or creates one if it doesn't exists (singleton)
fn get_client() -> &'static Client<OpenAIConfig> {
    CLIENT.get_or_init(Client::new)
}

/// Asynchronous OpenAI chat response, private in this project (since it's mostly sync)
async fn chat_response_async(
    system_prompt: &str,
    user_prompt: &str,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let client = get_client();

    let request = CreateChatCompletionRequestArgs::default()
        .model(CHAT_MODEL_DEFAULT)
        .reasoning_effort(ReasoningEffort::Low)
        .messages([
            ChatCompletionRequestSystemMessageArgs::default()
                .content(system_prompt)
                .build()?
                .into(),
            ChatCompletionRequestSystemMessageArgs::default()
                .content(user_prompt)
                .build()?
                .into(),
        ])
        .build()?;

    let response = client.chat().create(request).await?;

    let choice = response.choices.into_iter().next();

    match choice {
        Some(c) => Ok(c.message.content),
        None => Err("No choices in OpenAI chat completion object.".into()),
    }
}
