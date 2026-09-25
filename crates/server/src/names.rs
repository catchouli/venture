//! Random fantasy names, offered to new players.

const FIRST: &[&str] = &[
    "Al", "Bran", "Cor", "Da", "El", "Fen", "Gar", "Hal", "Is", "Jor", "Ka", "Lor", "Mer", "Na", "Os", "Per", "Ro",
    "Sa", "Tor", "Wyn",
];
const MIDDLE: &[&str] = &["", "", "a", "e", "i", "o", "ra", "li"];
const LAST: &[&str] =
    &["dric", "wen", "mund", "ra", "lin", "gar", "wyn", "ric", "na", "thor", "dis", "mir", "red", "via"];

/// A random name such as "Branwen" or "Tormir".
pub fn random_name() -> String {
    let pick = |parts: &[&'static str]| parts[fastrand::usize(..parts.len())];
    format!("{}{}{}", pick(FIRST), pick(MIDDLE), pick(LAST))
}
