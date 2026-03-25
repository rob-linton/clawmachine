//! Chat summary generation using `claude -p` with Haiku model.
//! Reuses the existing Claude Code CLI — no separate API client needed.
//! For chat sessions with a persistent container, runs via `docker exec`.

use tokio::process::Command;

/// Generate a one-line summary of a chat exchange using `claude -p` with Haiku.
/// `container_name` is the Docker container to exec into (None = run locally).
/// Returns None if the call fails.
pub async fn generate_summary(
    container_name: Option<&str>,
    user_message: &str,
    assistant_response: &str,
) -> Option<String> {
    let prompt = format!(
        "Summarize this chat exchange in ONE short sentence (max 100 chars). \
         Focus on what was discussed/decided, not meta-commentary. \
         Output ONLY the summary, nothing else.\n\n\
         User: {}\n\nAssistant: {}",
        truncate(user_message, 2000),
        truncate(assistant_response, 2000),
    );

    let output = match container_name {
        Some(name) => {
            // Run inside existing session container
            Command::new("docker")
                .args(["exec", name, "claude", "-p", &prompt, "--model", "haiku", "--output-format", "text"])
                .output()
                .await
                .ok()?
        }
        None => {
            // Run locally (for local execution backend)
            Command::new("claude")
                .args(["-p", &prompt, "--model", "haiku", "--output-format", "text"])
                .output()
                .await
                .ok()?
        }
    };

    if !output.status.success() {
        tracing::warn!(
            exit_code = ?output.status.code(),
            "Summary generation failed"
        );
        return None;
    }

    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn truncate(s: &str, max_chars: usize) -> &str {
    if s.len() <= max_chars {
        s
    } else {
        let mut end = max_chars;
        while !s.is_char_boundary(end) && end > 0 {
            end -= 1;
        }
        &s[..end]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("hello world", 5), "hello");
    }
}
