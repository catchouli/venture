//! What's allowed in names, chat messages and tokens. The server enforces
//! these rules; the client uses them too, to give instant feedback.

/// Fewest characters in a character name.
pub const MIN_NAME_CHARS: usize = 2;
/// Most characters in a character name.
pub const MAX_NAME_CHARS: usize = 20;
/// Most characters in one chat message.
pub const MAX_CHAT_CHARS: usize = 200;

/// Tokens must be this many characters or more, so they can't be guessed.
const MIN_TOKEN_CHARS: usize = 32;
/// And no longer than this.
const MAX_TOKEN_CHARS: usize = 128;

/// Tidies a requested character name, or explains why it isn't allowed.
/// Names are letters, spaces, apostrophes and hyphens, and start with a letter.
/// Runs of spaces are collapsed.
pub fn clean_name(name: &str) -> Result<String, String> {
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    let length = name.chars().count();
    if !(MIN_NAME_CHARS..=MAX_NAME_CHARS).contains(&length) {
        return Err(format!("Names must be {MIN_NAME_CHARS} to {MAX_NAME_CHARS} characters long."));
    }
    if !name.chars().next().is_some_and(char::is_alphabetic) {
        return Err("Names must start with a letter.".to_string());
    }
    if !name.chars().all(|c| c.is_alphabetic() || c == ' ' || c == '\'' || c == '-') {
        return Err("Names can only contain letters, spaces, apostrophes and hyphens.".to_string());
    }
    Ok(name)
}

/// Tidies a chat message: removes control characters, trims it and caps its
/// length. Returns `None` if nothing is left to say.
pub fn clean_chat_text(text: &str) -> Option<String> {
    let cleaned: String = text.chars().filter(|c| !c.is_control()).take(MAX_CHAT_CHARS).collect();
    let cleaned = cleaned.trim();
    (!cleaned.is_empty()).then(|| cleaned.to_string())
}

/// Whether a login token looks valid: long enough to be unguessable, and only
/// letters, digits and hyphens.
pub fn is_valid_token(token: &str) -> bool {
    (MIN_TOKEN_CHARS..=MAX_TOKEN_CHARS).contains(&token.len())
        && token.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_tidied_and_checked() {
        assert_eq!(clean_name("  Aldric   of  Millbrook "), Ok("Aldric of Millbrook".to_string()));
        assert_eq!(clean_name("O'Brien-Smith"), Ok("O'Brien-Smith".to_string()));
        assert!(clean_name("A").is_err());
        assert!(clean_name("'Quote").is_err());
        assert!(clean_name("Robert'); DROP TABLE").is_err());
        assert!(clean_name(&"x".repeat(MAX_NAME_CHARS + 1)).is_err());
    }

    #[test]
    fn chat_is_cleaned() {
        assert_eq!(clean_chat_text("  hello\u{7}\n "), Some("hello".to_string()));
        assert_eq!(clean_chat_text("   "), None);
        assert_eq!(clean_chat_text(&"a".repeat(1000)).unwrap().chars().count(), MAX_CHAT_CHARS);
    }

    #[test]
    fn tokens_are_checked() {
        assert!(is_valid_token(&"a1-".repeat(12)));
        assert!(!is_valid_token("short"));
        assert!(!is_valid_token(&format!("{}/../etc", "a".repeat(40))));
    }
}
