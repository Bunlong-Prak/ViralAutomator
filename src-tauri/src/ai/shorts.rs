use reqwest::Client;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct VideoSegment {
  pub start_time: String,
  pub end_time: String,
  pub virality_score: u8,
  pub title: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct DeepseekMessage {
  role: String,
  content: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct DeepseekRequest {
  model: String,
  messages: Vec<DeepseekMessage>,
  response_format: Option<ResponseFormat>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ResponseFormat {
  #[serde(rename = "type")]
  format_type: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct DeepseekResponse {
  choices: Vec<Choice>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Choice {
  message: MessageContent,
}

#[derive(Debug, Serialize, Deserialize)]
struct MessageContent {
  content: String,
}

pub async fn identify_viral_segments(
  api_key: &str,
  transcript: &str,
  num_clips: usize,
) -> Result<Vec<VideoSegment>, String> {
  let client = Client::new();

  let system_prompt = format!(
        "You are an expert viral video editor. Analyze the provided transcript and identify the top {} most engaging, viral segments. \
        Each segment should be between 30 and 60 seconds long. \
        Return ONLY a JSON array of objects with the following keys: start_time, end_time, virality_score (1-100), and title (a catchy hook). \
        Timestamps MUST be in HH:MM:SS format.",
        num_clips
    );

  let request = DeepseekRequest {
    model: "deepseek-chat".to_string(),
    messages: vec![
      DeepseekMessage {
        role: "system".to_string(),
        content: system_prompt,
      },
      DeepseekMessage {
        role: "user".to_string(),
        content: transcript.to_string(),
      },
    ],
    response_format: Some(ResponseFormat {
      format_type: "json_object".to_string(),
    }),
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
    .map_err(|e| format!("Failed to parse response: {}", e))?;

  let content = data
    .choices
    .get(0)
    .ok_or("No response from Deepseek")?
    .message
    .content
    .clone();

  // The model might wrap the array in an object if forced to json_object, or just return the array.
  // We try to find the array.
  let segments: Vec<VideoSegment> = if content.trim().starts_with('{') {
    let v: serde_json::Value = serde_json::from_str(&content).map_err(|e| e.to_string())?;
    if let Some(arr) = v
      .get("segments")
      .or_else(|| v.as_object().and_then(|m| m.values().next()))
      .and_then(|v| v.as_array())
    {
      serde_json::from_value(serde_json::Value::Array(arr.clone())).map_err(|e| e.to_string())?
    } else {
      return Err("Could not find segments array in JSON response".to_string());
    }
  } else {
    serde_json::from_str(&content).map_err(|e| e.to_string())?
  };

  Ok(segments)
}
