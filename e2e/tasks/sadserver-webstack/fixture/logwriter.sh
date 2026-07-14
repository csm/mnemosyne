#!/usr/bin/env bash
# Simulates a cron'd log writer: appends a line every few seconds.
mkdir -p /var/log/webstack
while true; do
    echo "$(date -u +%Y-%m-%dT%H:%M:%SZ) heartbeat ok" >> /var/log/webstack/writer.log
    sleep 5
done
