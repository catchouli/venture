Venture: the editor and the game (test build)
=============================================

Unzip this folder anywhere you like (not inside Program Files), and keep
everything in it together.

The editor
----------
Double-click venture_editor.exe.

It edits the files in this folder: content (classes, items, monsters, NPCs
and so on) and world_authoring (walls, buildings and bridges). Changes are
saved with Save (or Ctrl+S).

- "Save and play here" (World tab): starts the game on your own computer,
  with your changes, where the editor's camera is looking. You make a new
  character each time, with admin powers, so commands like /setlevel 5,
  /give stone_axe and /tp work. (Type /help in chat for the list.)
- "Save and bake" (World tab): builds the world again with your walls,
  buildings and bridges (a few seconds). The game then shows them.

The game
--------
- venture_client.exe: plays on the shared test server.
- venture_client.exe --local --admin: plays on your own computer, with this
  folder's content and world (from a command prompt in this folder).

Your test characters on your own computer are kept in local_data, in this
folder. Your login for the shared server is kept in your user folder, in .venture.

If Windows warns that the programs are unrecognised, choose "More info"
and then "Run anyway": they're unsigned test builds.
