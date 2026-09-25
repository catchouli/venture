//! What dialogue can do to the game: the **commands** a Yarn line can run
//! (`<<give_item bread 2>>`) and the **functions** its conditions and text can
//! call (`item_count("bread")`).
//!
//! Commands only name things; the server checks and carries them out (see
//! `crates/server/src/dialogue.rs`). The content check makes sure every
//! command and function in the dialogue exists, with the right arguments,
//! and refers to items and shops that exist.

/// A command dialogue can run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DialogueCommand {
    /// `<<give_item ITEM [COUNT]>>`: put items in the player's bag.
    GiveItem {
        /// Which item.
        item: String,
        /// How many (1 if not given).
        count: u32,
    },
    /// `<<take_item ITEM [COUNT]>>`: take items from the player's bag. Check
    /// they have them first, with `item_count`.
    TakeItem {
        /// Which item.
        item: String,
        /// How many (1 if not given).
        count: u32,
    },
    /// `<<open_shop SHOP>>`: end the conversation and open a shop. It ends
    /// at once, so any lines before it on the same page aren't shown.
    OpenShop {
        /// Which shop.
        shop: String,
    },
}

/// The functions the game provides to dialogue, on top of Yarn Spinner's own.
pub const DIALOGUE_FUNCTIONS: [&str; 2] = [
    // player_name(): the name of the player in the conversation.
    "player_name",
    // item_count(item): how many of an item the player carries.
    "item_count",
];

impl DialogueCommand {
    /// Reads a command from the text of a Yarn command (what's between `<<` and `>>`).
    pub fn parse(text: &str) -> Result<Self, String> {
        if text.contains('{') {
            return Err(format!("<<{text}>>: commands can't use {{expressions}} yet"));
        }
        let words: Vec<&str> = text.split_whitespace().collect();
        let count = |word: Option<&&str>| -> Result<u32, String> {
            match word {
                None => Ok(1),
                Some(word) => match word.parse::<u32>() {
                    Ok(count) if count > 0 => Ok(count),
                    _ => Err(format!("<<{text}>>: \"{word}\" isn't a count (a whole number from 1)")),
                },
            }
        };
        match words.as_slice() {
            ["give_item", item] | ["give_item", item, _] => {
                Ok(Self::GiveItem { item: item.to_string(), count: count(words.get(2))? })
            }
            ["take_item", item] | ["take_item", item, _] => {
                Ok(Self::TakeItem { item: item.to_string(), count: count(words.get(2))? })
            }
            ["open_shop", shop] => Ok(Self::OpenShop { shop: shop.to_string() }),
            ["give_item" | "take_item", ..] => Err(format!("<<{text}>>: write it as <<{} ITEM [COUNT]>>", words[0])),
            ["open_shop", ..] => Err(format!("<<{text}>>: write it as <<open_shop SHOP>>")),
            _ => Err(format!("<<{text}>> isn't a command the game knows (commands: give_item, take_item, open_shop)")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_are_read() {
        let give = DialogueCommand::parse("give_item bread 2");
        assert_eq!(give, Ok(DialogueCommand::GiveItem { item: "bread".to_string(), count: 2 }));
        let take = DialogueCommand::parse("take_item old_pipe");
        assert_eq!(take, Ok(DialogueCommand::TakeItem { item: "old_pipe".to_string(), count: 1 }));
        let shop = DialogueCommand::parse("open_shop elsies_bakery");
        assert_eq!(shop, Ok(DialogueCommand::OpenShop { shop: "elsies_bakery".to_string() }));
    }

    #[test]
    fn mistakes_are_explained() {
        assert!(DialogueCommand::parse("give_item bread 0").unwrap_err().contains("isn't a count"));
        assert!(DialogueCommand::parse("give_item").unwrap_err().contains("write it as"));
        assert!(DialogueCommand::parse("fly away").unwrap_err().contains("isn't a command"));
        assert!(DialogueCommand::parse("give_item {$thing}").unwrap_err().contains("expressions"));
    }
}
