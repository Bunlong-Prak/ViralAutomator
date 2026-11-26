/// Generates viral Hormozi-style word-by-word animated ASS subtitles and 3-second hook banners
/// sized for 1080x1920 vertical short-form video (YouTube Shorts, TikTok, Instagram Reels).

#[derive(Debug, Clone, PartialEq)]
pub struct SrtCue {
  pub start_secs: f64,
  pub end_secs: f64,
  pub text: String,
}

pub fn parse_timestamp_secs(ts: &str) -> Option<f64> {
  let clean = ts.trim().replace(',', ".");
  let parts: Vec<&str> = clean.split(':').collect();
  match parts.len() {
    3 => {
      let h: f64 = parts[0].trim().parse().ok()?;
      let m: f64 = parts[1].trim().parse().ok()?;
      let s: f64 = parts[2].trim().parse().ok()?;
      Some(h * 3600.0 + m * 60.0 + s)
    }
    2 => {
      let m: f64 = parts[0].trim().parse().ok()?;
      let s: f64 = parts[1].trim().parse().ok()?;
      Some(m * 60.0 + s)
    }
    1 => parts[0].trim().parse().ok(),
    _ => None,
  }
}

pub fn format_ass_time(secs: f64) -> String {
  let clamped = secs.max(0.0);
  let total_centis = (clamped * 100.0).round() as u64;
  let cs = total_centis % 100;
  let total_secs = total_centis / 100;
  let s = total_secs % 60;
  let total_mins = total_secs / 60;
  let m = total_mins % 60;
  let h = total_mins / 60;
  format!("{}:{:02}:{:02}.{:02}", h, m, s, cs)
}

pub fn parse_srt_cues(srt: &str) -> Vec<SrtCue> {
  let normalized = srt.replace("\r\n", "\n");
  let mut cues = Vec::new();

  for block in normalized.split("\n\n") {
    let lines: Vec<&str> = block
      .lines()
      .map(|l| l.trim())
      .filter(|l| !l.is_empty())
      .collect();
    if lines.len() < 2 {
      continue;
    }

    let Some(time_pos) = lines.iter().position(|l| l.contains("-->")) else {
      continue;
    };
    let parts: Vec<&str> = lines[time_pos].split("-->").collect();
    if parts.len() != 2 {
      continue;
    }
    let (Some(start), Some(end)) = (
      parse_timestamp_secs(parts[0]),
      parse_timestamp_secs(parts[1]),
    ) else {
      continue;
    };

    let text = lines[time_pos + 1..].join(" ").trim().to_string();
    if !text.is_empty() && end > start {
      cues.push(SrtCue {
        start_secs: start,
        end_secs: end,
        text,
      });
    }
  }

  cues
}

pub fn escape_ass_text(input: &str) -> String {
  input
    .replace('\\', r"\\")
    .replace('{', r"\{")
    .replace('}', r"\}")
    .replace('\n', r"\N")
}

/// Breaks words into punchy 2-4 word short-form caption chunks (max ~18 chars).
fn chunk_words(words: &[&str]) -> Vec<Vec<String>> {
  let mut chunks: Vec<Vec<String>> = Vec::new();
  let mut current: Vec<String> = Vec::new();
  let mut current_len = 0usize;

  for word in words {
    let w = word.trim();
    if w.is_empty() {
      continue;
    }
    let word_len = w.chars().count();
    let should_split = !current.is_empty()
      && (current.len() >= 3 || (current_len + 1 + word_len > 18 && current.len() >= 2));
    if should_split {
      chunks.push(current);
      current = Vec::new();
      current_len = 0;
    }
    if !current.is_empty() {
      current_len += 1;
    }
    current_len += word_len;
    current.push(w.to_uppercase());
  }

  if !current.is_empty() {
    chunks.push(current);
  }
  chunks
}

/// Builds a complete ASS subtitle document for 1080x1920 vertical videos with:
/// - Optional 3-second viral hook banner pinned in the upper safe zone (0:00 -> 0:03.2)
/// - Word-by-word highlighted Hormozi-style dynamic captions in the lower-middle safe zone
pub fn build_hormozi_ass(
  srt_content: &str,
  hook_banner: Option<&str>,
  include_captions: bool,
) -> String {
  let mut ass = String::from(
    "[Script Info]\n\
    ScriptType: v4.00+\n\
    PlayResX: 1080\n\
    PlayResY: 1920\n\
    ScaledBorderAndShadow: yes\n\
    WrapStyle: 2\n\n\
    [V4+ Styles]\n\
    Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n\
    Style: Hormozi,Arial,82,&H00FFFFFF,&H0000E5FF,&H00000000,&H96000000,-1,0,0,0,100,100,1,0,1,8,4,2,70,70,440,1\n\
    Style: HookBanner,Arial,66,&H00101010,&H00101010,&H0000E5FF,&H0000E5FF,-1,0,0,0,100,100,1,0,3,22,0,8,90,90,230,1\n\n\
    [Events]\n\
    Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n",
  );

  // 1. 3-second viral hook banner (high contrast bold yellow pill at top safe zone)
  if let Some(hook) = hook_banner {
    let cleaned = hook.trim().to_uppercase();
    if !cleaned.is_empty() {
      let escaped = escape_ass_text(&cleaned);
      ass.push_str(&format!(
        "Dialogue: 2,0:00:00.00,0:00:03.20,HookBanner,,0,0,0,,{{\\an8\\fscx88\\fscy88\\t(0,160,\\fscx104\\fscy104)\\t(160,280,\\fscx100\\fscy100)\\fad(0,250)}}{}\n",
        escaped
      ));
    }
  }

  // 2. Word-by-word active-highlight Hormozi captions
  if include_captions {
    let cues = parse_srt_cues(srt_content);
    for cue in cues {
      let words: Vec<&str> = cue.text.split_whitespace().collect();
      if words.is_empty() {
        continue;
      }

      let chunks = chunk_words(&words);
      if chunks.is_empty() {
        continue;
      }

      let total_chars: usize = chunks
        .iter()
        .map(|c| c.iter().map(|w| w.chars().count().max(1)).sum::<usize>())
        .sum::<usize>()
        .max(1);

      let cue_duration = (cue.end_secs - cue.start_secs).max(0.2);
      let mut cursor = cue.start_secs;

      for chunk in chunks {
        let chunk_chars: usize = chunk
          .iter()
          .map(|w| w.chars().count().max(1))
          .sum::<usize>()
          .max(1);
        let chunk_duration = (cue_duration * (chunk_chars as f64 / total_chars as f64)).max(0.15);
        let chunk_end = (cursor + chunk_duration).min(cue.end_secs);

        // Step through each word in the chunk, highlighting the spoken word in bright viral yellow
        let mut word_cursor = cursor;
        for (idx, _) in chunk.iter().enumerate() {
          let w_chars = chunk[idx].chars().count().max(1);
          let w_dur = (chunk_duration * (w_chars as f64 / chunk_chars as f64)).max(0.06);
          let w_end = if idx + 1 == chunk.len() {
            chunk_end.max(word_cursor + 0.05)
          } else {
            (word_cursor + w_dur).min(chunk_end)
          };

          let mut line_text = String::new();
          // Subtle pop-in on first word of chunk
          if idx == 0 {
            line_text.push_str(r"{\fscx92\fscy92\t(0,90,\fscx100\fscy100)}");
          }

          for (j, word) in chunk.iter().enumerate() {
            if j > 0 {
              line_text.push(' ');
            }
            let escaped = escape_ass_text(word);
            if j == idx {
              // Active word: vibrant viral yellow (&H0000E5FF in ASS BGR) + subtle zoom emphasis
              line_text.push_str(&format!(
                "{{\\c&H0000E5FF&\\fscx112\\fscy112}}{}{{\\c&H00FFFFFF&\\fscx100\\fscy100}}",
                escaped
              ));
            } else {
              line_text.push_str(&escaped);
            }
          }

          ass.push_str(&format!(
            "Dialogue: 1,{},{},Hormozi,,0,0,0,,{}\n",
            format_ass_time(word_cursor),
            format_ass_time(w_end),
            line_text
          ));

          word_cursor = w_end;
        }

        cursor = chunk_end;
      }
    }
  }

  ass
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn parse_srt_cues_extracts_valid_blocks() {
    let srt = "1\n00:00:01,000 --> 00:00:03,500\nThis changes everything\n\n2\n00:00:04,000 --> 00:00:06,000\nWatch until the end\n";
    let cues = parse_srt_cues(srt);
    assert_eq!(cues.len(), 2);
    assert!((cues[0].start_secs - 1.0).abs() < 1e-3);
    assert_eq!(cues[0].text, "This changes everything");
  }

  #[test]
  fn build_hormozi_ass_includes_hook_and_word_highlights() {
    let srt = "1\n00:00:00,500 --> 00:00:02,000\nStop doing this\n";
    let ass = build_hormozi_ass(srt, Some("WAIT FOR THIS"), true);
    assert!(ass.contains("PlayResX: 1080"));
    assert!(ass.contains("PlayResY: 1920"));
    assert!(ass.contains("HookBanner"));
    assert!(ass.contains("WAIT FOR THIS"));
    assert!(ass.contains("0:00:03.20"));
    assert!(ass.contains(r"\c&H0000E5FF&"));
    assert!(ass.contains("STOP"));
    assert!(ass.contains("DOING"));
    assert!(ass.contains("THIS"));
  }

  #[test]
  fn build_hormozi_ass_hook_only_without_captions() {
    let ass = build_hormozi_ass("", Some("BIG SECRET"), false);
    assert!(ass.contains("BIG SECRET"));
    assert!(!ass.contains("Hormozi,,"));
  }
}
