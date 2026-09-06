#!/usr/bin/env bash
# A real window manager supplies native activation and keyboard focus under Xvfb.
set -euo pipefail
openbox >/dev/null 2>&1 &
window_manager=$!
trap 'kill "$window_manager" 2>/dev/null || true' EXIT
python scripts/gui_webdriver.py --binary /usr/bin/autoharness \
  --native-driver /usr/bin/WebKitWebDriver \
  --output target/gui-evidence/linux
