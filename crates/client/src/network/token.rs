//! The login token: a long random string identifying this player to the
//! server (see `docs/client-server.md`). Created on first run and kept in the
//! browser's `localStorage`, or in a file on desktop.

/// Where the token is kept.
const STORAGE_KEY: &str = "venture_token";

/// The saved token, or a new one if there isn't one yet. `slot` selects a
/// separate token (such as for `--local` mode, or a named test profile).
pub fn load_or_create(slot: Option<&str>) -> String {
    let name = match slot {
        Some(slot) => format!("{STORAGE_KEY}_{slot}"),
        None => STORAGE_KEY.to_string(),
    };
    if let Some(token) = load(&name).filter(|token| venture_protocol::is_valid_token(token)) {
        return token;
    }
    let token = new_token();
    save(&name, &token);
    token
}

/// A new random token: 32 random bytes as hex.
fn new_token() -> String {
    random_bytes().iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(target_arch = "wasm32")]
fn random_bytes() -> [u8; 32] {
    let mut bytes = [0; 32];
    let crypto = web_sys::window().and_then(|window| window.crypto().ok()).expect("the browser has crypto");
    crypto.get_random_values_with_u8_array(&mut bytes).expect("the browser provides random numbers");
    bytes
}

#[cfg(not(target_arch = "wasm32"))]
fn random_bytes() -> [u8; 32] {
    use std::hash::{BuildHasher, Hasher};
    // Each `RandomState` is seeded randomly by the operating system.
    let mut bytes = [0; 32];
    for chunk in bytes.chunks_mut(8) {
        let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
        hasher.write_u64(0);
        chunk.copy_from_slice(&hasher.finish().to_le_bytes());
    }
    bytes
}

#[cfg(target_arch = "wasm32")]
fn load(name: &str) -> Option<String> {
    web_sys::window()?.local_storage().ok()??.get_item(name).ok()?
}

#[cfg(target_arch = "wasm32")]
fn save(name: &str, token: &str) {
    if let Some(storage) = web_sys::window().and_then(|window| window.local_storage().ok().flatten()) {
        let _ = storage.set_item(name, token);
    }
}

/// The folder desktop tokens are kept in.
#[cfg(not(target_arch = "wasm32"))]
fn folder() -> std::path::PathBuf {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).unwrap_or_default();
    std::path::PathBuf::from(home).join(".venture")
}

#[cfg(not(target_arch = "wasm32"))]
fn load(name: &str) -> Option<String> {
    std::fs::read_to_string(folder().join(name)).ok().map(|token| token.trim().to_string())
}

#[cfg(not(target_arch = "wasm32"))]
fn save(name: &str, token: &str) {
    let folder = folder();
    let _ = std::fs::create_dir_all(&folder).and_then(|()| std::fs::write(folder.join(name), token));
}
