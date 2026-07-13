"""Fault catalog for sadserver-webstack. NEVER copied into any
agent-reachable image (see e2e/README.md "Anti-cheating") -- only
fixture/compose_break.py (host-side, run by fixture/prepare.sh before the
Docker build) imports this, and it only ever emits the *shell body* of the
chosen faults into fixture/break.sh, never a fault's name or description.

Each fault is a self-contained shell fragment, run once (as root) at
container start, before nginx/the app/the log writer come up. All faults
assume the same fixture layout:

  /etc/webstack/app.env      -- PORT=5000, ITEMS_DB_PATH=/data/items.db
  /etc/nginx/conf.d/app.conf -- proxy_pass http://127.0.0.1:5000;
  /data/items.db             -- sqlite3 db, created on first app start

A correct fix must survive `restart-services.sh` (see fixture/), i.e. it
has to change on-disk config/permissions, not just patch a running process.
"""

FAULT_CATALOG = {
    "nginx_wrong_upstream_port": """
sed -i 's/proxy_pass http:\\/\\/127\\.0\\.0\\.1:5000;/proxy_pass http:\\/\\/127.0.0.1:5001;/' \
    /etc/nginx/conf.d/app.conf
""",
    "env_file_corrupted": """
cat > /etc/webstack/app.env <<'FAULTEOF'
PORT=5000
ITEMS_DB_PATH
FAULTEOF
""",
    "disk_nearly_full": """
mkdir -p /data
fallocate -l "$(df --output=avail -B1 /data | tail -1 | awk '{print $1 - 8*1024*1024}')" \
    /data/.ballast 2>/dev/null || \
  dd if=/dev/zero of=/data/.ballast bs=1M count=$(( $(df --output=avail -B1 /data | tail -1) / 1024 / 1024 - 8 )) 2>/dev/null
""",
    "db_file_wrong_permissions": """
mkdir -p /data
: > /data/items.db
chmod 000 /data/items.db
""",
}

FAULT_NAMES = sorted(FAULT_CATALOG.keys())
