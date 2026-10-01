#!/bin/sh
# Packages the editor and the game for Windows (cross-compiled, from a Mac
# or Linux), with the content and a baked world, as dist/Venture-editor-windows-x64.zip.
# See "A packaged editor" in docs/editor.md.
#
#     tools/package_windows.sh [server address]
#
# The server address is where the game connects without --connect (such
# as http://87.74.253.246:8080); the editor's "Save and play here" plays
# locally either way. Needs MinGW-w64 (brew install mingw-w64) and
# `rustup target add x86_64-pc-windows-gnu`, and the world baked (and the
# content's Git LFS files pulled).
set -eu

cd "$(dirname "$0")/.."
target=x86_64-pc-windows-gnu
out=dist/Venture-editor
server="${1:-}"

if [ -n "$server" ]; then
    VENTURE_SERVER="$server" cargo build --release --target "$target" -p venture_client
else
    cargo build --release --target "$target" -p venture_client
fi
cargo build --release --target "$target" -p venture_editor
cargo build --release --target "$target" -p venture_worldgen --bin bake

rm -rf "$out" "$out-windows-x64.zip"
mkdir -p "$out"
for program in venture_editor venture_client bake; do
    x86_64-w64-mingw32-strip -o "$out/$program.exe" "target/$target/release/$program.exe"
done
cp -R content world_authoring world "$out/"
cp server.ron "$out/"
cp tools/package_windows_readme.txt "$out/README.txt"
(cd dist && zip -qr9 "Venture-editor-windows-x64.zip" Venture-editor)
ls -la "$out-windows-x64.zip"
