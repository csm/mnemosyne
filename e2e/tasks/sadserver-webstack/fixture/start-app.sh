#!/usr/bin/env bash
set -a
source /etc/webstack/app.env
set +a
exec python3 /opt/webstack/app.py >>/var/log/webstack/app.log 2>&1
