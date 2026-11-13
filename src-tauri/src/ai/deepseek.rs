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

  let (endpoint, model_name) = if api_key.starts_with("gsk_") {
    (
      "https://api.groq.com/openai/v1/chat/completions",
      "llama-3.3-70b-versatile",
    )
  } else {
    ("https://api.deepseek.com/chat/completions", "deepseek-chat")
  };

  let request = DeepseekRequest {
    model: model_name.to_string(),
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
    .post(endpoint)
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
    .first()
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
    #[serde(alias = "Title")]
    title: Option<String>,
    #[serde(alias = "Description", alias = "desc", alias = "Desc")]
    description: Option<String>,
  }

  let meta: Meta =
    serde_json::from_str(json_str).map_err(|e| format!("Failed to parse metadata: {}", e))?;

  let final_title = meta
    .title
    .filter(|t| !t.trim().is_empty())
    .unwrap_or_else(|| original_title.to_string());
  let final_desc = meta
    .description
    .unwrap_or_else(|| format!("{} #shorts", final_title));

  Ok((final_title, final_desc))
}

pub async fn generate_translation(
  api_key: &str,
  text: &str,
  system_prompt: &str,
) -> Result<String, String> {
  let client = reqwest::Client::new();

  let (endpoint, model_name) = if api_key.starts_with("gsk_") {
    (
      "https://api.groq.com/openai/v1/chat/completions",
      "llama-3.3-70b-versatile",
    )
  } else {
    ("https://api.deepseek.com/chat/completions", "deepseek-chat")
  };

  let request = DeepseekRequest {
    model: model_name.to_string(),
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
    .post(endpoint)
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
      .first()
      .ok_or("No choices returned")?
      .message
      .content
      .clone(),
  )
}
