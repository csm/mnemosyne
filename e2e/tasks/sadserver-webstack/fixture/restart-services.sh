#!/usr/bin/env bash
# Restarts nginx and the app in place, without killing the container's PID 1
# -- used by ../grader/grade.py (via `docker exec`) to confirm a fix
# survives a restart rather than just patching the running processes.
set -e

pkill -f "python3 /opt/webstack/app.py" 2>/dev/null || true
sleep 1
/usr/local/bin/start-app.sh &
sleep 1

nginx -s reload
sleep 1
