//! Checks a content pack for mistakes.
//!
//! ```text
//! cargo run -p venture_content --features scripts --bin check -- [content folder]
//! ```

use std::path::PathBuf;
use std::process::ExitCode;

use venture_content::ContentPack;

fn main() -> ExitCode {
    let folder = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "content".to_string()));
    match ContentPack::load(&folder) {
        Ok(pack) => {
            println!(
                "{} is fine: {} appearances, {} NPCs, {} scripts, {} dialogue files.",
                folder.display(),
                pack.appearances.len(),
                pack.npcs.len(),
                pack.scripts.len(),
                pack.dialogue.len()
            );
            ExitCode::SUCCESS
        }
        Err(errors) => {
            for error in &errors {
                eprintln!("{error}");
            }
            eprintln!("{} problem(s) found.", errors.len());
            ExitCode::FAILURE
        }
    }
}
