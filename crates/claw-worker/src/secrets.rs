//! Render tool credentials as a sourceable bash payload that the worker
//! pipes to the container via stdin.
//!
//! The output is a series of `export KEY=$'...'` lines using ANSI-C ($'...')
//! quoting so any byte (including newlines, single quotes, backslashes,
//! non-UTF-8) round-trips safely. Keys that aren't valid POSIX env var names
//! are filtered out — this defends against credential-store corruption
//! causing `eval` failures inside the container.
//!
//! Output is sorted by key for deterministic test output.

use std::collections::HashMap;

/// Render the credentials map as a bash-sourceable payload. Empty input
/// produces an empty string.
pub fn render_credentials_for_stdin(creds: &HashMap<String, String>) -> String {
    let mut keys: Vec<&String> = creds.keys().filter(|k| is_valid_env_key(k)).collect();
    keys.sort();
    let mut out = String::new();
    for key in keys {
        // Safe to unwrap — the key came from `creds.keys()`
        let value = creds.get(key).unwrap();
        out.push_str("export ");
        out.push_str(key);
        out.push('=');
        out.push_str(&ansi_c_quote(value));
        out.push('\n');
    }
    out
}

/// POSIX env var name: starts with letter or underscore, then letters,
/// digits, or underscores. Must be non-empty.
fn is_valid_env_key(key: &str) -> bool {
    let mut chars = key.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Produce the ANSI-C ($'...') quoted form of a value. Safe for any byte
/// sequence including newlines, NULs, quotes, backslashes, and non-UTF-8.
fn ansi_c_quote(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 4);
    out.push('$');
    out.push('\'');
    for byte in value.as_bytes() {
        match *byte {
            b'\'' => out.push_str("\\'"),
            b'\\' => out.push_str("\\\\"),
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            b'\t' => out.push_str("\\t"),
            // Printable ASCII (space..tilde) — emit as-is
            0x20..=0x7e => out.push(*byte as char),
            // Everything else (including NUL, control bytes, non-UTF-8) →
            // \xHH escape, which bash $'...' supports natively.
            other => out.push_str(&format!("\\x{:02x}", other)),
        }
    }
    out.push('\'');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    #[test]
    fn empty_map_produces_empty_string() {
        assert_eq!(render_credentials_for_stdin(&HashMap::new()), "");
    }

    #[test]
    fn single_simple_value() {
        let out = render_credentials_for_stdin(&map(&[("AWS_ACCESS_KEY_ID", "AKIA1234")]));
        assert_eq!(out, "export AWS_ACCESS_KEY_ID=$'AKIA1234'\n");
    }

    #[test]
    fn multiple_values_sorted_deterministically() {
        let out = render_credentials_for_stdin(&map(&[
            ("ZEBRA", "z"),
            ("ALPHA", "a"),
            ("MIDDLE", "m"),
        ]));
        assert_eq!(
            out,
            "export ALPHA=$'a'\nexport MIDDLE=$'m'\nexport ZEBRA=$'z'\n"
        );
    }

    #[test]
    fn value_with_newline() {
        let out = render_credentials_for_stdin(&map(&[("KEY", "line1\nline2")]));
        assert_eq!(out, "export KEY=$'line1\\nline2'\n");
    }

    #[test]
    fn value_with_single_quote() {
        let out = render_credentials_for_stdin(&map(&[("KEY", "it's")]));
        assert_eq!(out, "export KEY=$'it\\'s'\n");
    }

    #[test]
    fn value_with_backslash() {
        let out = render_credentials_for_stdin(&map(&[("KEY", "a\\b")]));
        assert_eq!(out, "export KEY=$'a\\\\b'\n");
    }

    #[test]
    fn value_with_tab_and_carriage_return() {
        let out = render_credentials_for_stdin(&map(&[("KEY", "a\tb\rc")]));
        assert_eq!(out, "export KEY=$'a\\tb\\rc'\n");
    }

    #[test]
    fn value_with_non_ascii_bytes_get_hex_escaped() {
        // ä is 0xc3 0xa4 in UTF-8 → both bytes are non-printable in ASCII range
        let out = render_credentials_for_stdin(&map(&[("KEY", "ä")]));
        assert_eq!(out, "export KEY=$'\\xc3\\xa4'\n");
    }

    #[test]
    fn empty_value_round_trips() {
        let out = render_credentials_for_stdin(&map(&[("KEY", "")]));
        assert_eq!(out, "export KEY=$''\n");
    }

    #[test]
    fn key_with_leading_underscore_is_valid() {
        let out = render_credentials_for_stdin(&map(&[("_PRIVATE", "x")]));
        assert_eq!(out, "export _PRIVATE=$'x'\n");
    }

    #[test]
    fn key_starting_with_digit_is_filtered() {
        let out = render_credentials_for_stdin(&map(&[("1BAD", "x"), ("GOOD", "y")]));
        assert_eq!(out, "export GOOD=$'y'\n");
    }

    #[test]
    fn key_with_special_char_is_filtered() {
        let out = render_credentials_for_stdin(&map(&[("BAD-KEY", "x"), ("GOOD", "y")]));
        assert_eq!(out, "export GOOD=$'y'\n");
    }

    #[test]
    fn empty_key_is_filtered() {
        let out = render_credentials_for_stdin(&map(&[("", "x"), ("GOOD", "y")]));
        assert_eq!(out, "export GOOD=$'y'\n");
    }

    #[test]
    fn key_validation() {
        assert!(is_valid_env_key("AWS_ACCESS_KEY_ID"));
        assert!(is_valid_env_key("_X"));
        assert!(is_valid_env_key("a"));
        assert!(is_valid_env_key("Foo123"));
        assert!(!is_valid_env_key(""));
        assert!(!is_valid_env_key("1FOO"));
        assert!(!is_valid_env_key("FOO-BAR"));
        assert!(!is_valid_env_key("FOO BAR"));
        assert!(!is_valid_env_key("FOO.BAR"));
    }

    #[test]
    fn ansi_c_quote_basics() {
        assert_eq!(ansi_c_quote(""), "$''");
        assert_eq!(ansi_c_quote("hello"), "$'hello'");
        assert_eq!(ansi_c_quote("a b"), "$'a b'");
    }

    #[test]
    fn realistic_aws_credential_payload() {
        // Sanity check on a payload close to what would actually be injected.
        let out = render_credentials_for_stdin(&map(&[
            ("AWS_ACCESS_KEY_ID", "AKIAIOSFODNN7EXAMPLE"),
            ("AWS_SECRET_ACCESS_KEY", "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY"),
            ("AWS_DEFAULT_REGION", "us-east-1"),
        ]));
        let expected = "\
export AWS_ACCESS_KEY_ID=$'AKIAIOSFODNN7EXAMPLE'
export AWS_DEFAULT_REGION=$'us-east-1'
export AWS_SECRET_ACCESS_KEY=$'wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY'
";
        assert_eq!(out, expected);
    }
}

/// Build the bash bootstrap that reads credentials from stdin and exports
/// them into the environment. Returns the prelude — caller appends the rest
/// of the runner script (auth lines, cd, exec claude).
///
/// The prelude is a no-op when there are no credentials to inject.
pub fn credential_load_prelude(has_creds: bool) -> &'static str {
    if !has_creds {
        return "";
    }
    // Note: 2>/dev/null on the eval ensures a malformed credential value
    // (which should never happen given the unit-tested renderer, but
    // defends against credential-store corruption) cannot leak the
    // offending text into the worker's captured stderr.
    "__claw_load_creds() {
  local __ev
  set +e
  eval \"$(cat /dev/stdin)\" 2>/dev/null
  __ev=$?
  set -e
  if [ $__ev -ne 0 ]; then
    echo \"claw: failed to load credentials\" >&2
    exit 1
  fi
}
__claw_load_creds
unset -f __claw_load_creds
"
}
