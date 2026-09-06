#!/usr/bin/env bash
# A real window manager supplies native activation and keyboard focus under Xvfb.
set -euo pipefail
# Xvfb has no physical GPU compositor; keep this CI-only rendering mode explicit.
# https://github.com/tauri-apps/tauri/issues/15936
export WEBKIT_DISABLE_COMPOSITING_MODE=1
export WEBKIT_DISABLE_DMABUF_RENDERER=1
export LIBGL_ALWAYS_SOFTWARE=1
openbox >/dev/null 2>&1 &
window_manager=$!
trap 'kill "$window_manager" 2>/dev/null || true' EXIT
python scripts/gui_webdriver.py --binary /usr/bin/autoharness \
  --native-driver /usr/bin/WebKitWebDriver \
  --output target/gui-evidence/linux
