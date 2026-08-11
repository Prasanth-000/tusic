#!/bin/bash
# KALKI — total uninstall. Removes project dir + the ~/.local/bin/music shim.
# One-command removal, no prompts.
set -e
PROJ="$HOME/music-player"
SHIM="$HOME/.local/bin/music"

echo "KALKI uninstall"
# stop any running instance + its mpv child
pkill -f "input-ipc-server=/tmp/kalki.sock" 2>/dev/null || true
pkill -f "$PROJ/target/release/kalki" 2>/dev/null || true
rm -f /tmp/kalki.sock

if [ -d "$PROJ" ]; then
  rm -rf "$PROJ"
  echo "  removed $PROJ"
fi
if [ -L "$SHIM" ] || [ -f "$SHIM" ]; then
  rm -f "$SHIM"
  echo "  removed $SHIM"
fi
echo "done."
