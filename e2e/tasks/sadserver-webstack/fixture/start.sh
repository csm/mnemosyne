#!/usr/bin/env bash
# Container entrypoint (PID 1 ends up as nginx, in the foreground).
set -e

mkdir -p /data /var/log/webstack

# Apply the seeded faults baked in at image build time (see
# ../../grader/fault_catalog.py and prepare.sh/compose_break.py).
if [ -x /usr/local/bin/break.sh ]; then
    /usr/local/bin/break.sh || true
fi

/usr/local/bin/logwriter.sh &
/usr/local/bin/start-app.sh &

exec nginx -g 'daemon off;'
