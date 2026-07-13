This box is running a small web stack: nginx (port 8080) reverse-proxying a
Flask API backed by SQLite, plus a background log writer.

`curl localhost:8080/health` should return HTTP 200 with body `{"status":
"ok"}`, and `POST /items` (JSON body `{"name": "..."}`)  should persist and
show up in `GET /items`. It doesn't work right now.

Diagnose and fix whatever is wrong. Your fix must survive a service
restart, not just patch things at runtime -- assume config/permission
changes need to be on disk, not just applied to the currently running
processes.
