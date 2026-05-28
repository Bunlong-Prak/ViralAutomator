use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
struct DeepseekRequest {
  model: String,
  messages: Vec<Message>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Message {
  role: String,
  content: String,
}

#[derive(Debug, Deserialize)]
struct DeepseekResponse {
  choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
  message: Message,
}

pub async fn generate_metadata(
  api_key: &str,
  original_title: &str,
) -> Result<(String, String), String> {
  let client = reqwest::Client::new();

  let prompt = format!(
        "Generate a catchy viral title and a short description for a 9:16 video with the original title: '{}'. Return only a JSON object with 'title' and 'description' keys.",
        original_title
    );

  let request = DeepseekRequest {
    model: "deepseek-chat".to_string(),
    messages: vec![
      Message {
        role: "system".to_string(),
        content: "You are a social media expert.".to_string(),
      },
      Message {
        role: "user".to_string(),
        content: prompt,
      },
    ],
  };

  let response = client
    .post("https://api.deepseek.com/v1/chat/completions")
    .header("Authorization", format!("Bearer {}", api_key))
    .json(&request)
    .send()
    .await
    .map_err(|e| format!("Request failed: {}", e))?;

  if !response.status().is_success() {
    let status = response.status();
    let error_body = response.text().await.unwrap_or_default();
    return Err(format!("Deepseek API error: {} - {}", status, error_body));
  }

  let data: DeepseekResponse = response
    .json()
    .await
    .map_err(|e| format!("Failed to parse JSON: {}", e))?;

  let content = data
    .choices
    .get(0)
    .ok_or("No choices returned")?
    .message
    .content
    .clone();

  // Simple parsing of JSON from text
  let start = content.find('{').ok_or("No JSON found")?;
  let end = content.rfind('}').ok_or("No JSON found")? + 1;
  let json_str = &content[start..end];

  #[derive(Deserialize)]
  struct Meta {
    title: String,
    description: String,
  }

  let meta: Meta =
    serde_json::from_str(json_str).map_err(|e| format!("Failed to parse metadata: {}", e))?;

  Ok((meta.title, meta.description))
}

pub async fn generate_translation(
  api_key: &str,
  text: &str,
  system_prompt: &str,
) -> Result<String, String> {
  let client = reqwest::Client::new();

  let request = DeepseekRequest {
    model: "deepseek-chat".to_string(),
    messages: vec![
      Message {
        role: "system".to_string(),
        content: system_prompt.to_string(),
      },
      Message {
        role: "user".to_string(),
        content: text.to_string(),
      },
    ],
  };

  let response = client
    .post("https://api.deepseek.com/v1/chat/completions")
    .header("Authorization", format!("Bearer {}", api_key))
    .json(&request)
    .send()
    .await
    .map_err(|e| format!("Request failed: {}", e))?;

  if !response.status().is_success() {
    let status = response.status();
    let error_body = response.text().await.unwrap_or_default();
    return Err(format!("Deepseek API error: {} - {}", status, error_body));
  }

  let data: DeepseekResponse = response
    .json()
    .await
    .map_err(|e| format!("Failed to parse JSON: {}", e))?;

  Ok(
    data
      .choices
      .get(0)
      .ok_or("No choices returned")?
      .message
      .content
      .clone(),
  )
}
