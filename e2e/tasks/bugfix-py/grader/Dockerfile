# Grader image: only pytest/flask baked in at build time (network available
# here). At grade time run.sh runs this image with --network none and
# bind-mounts this directory read-only at /grader, so this Dockerfile does
# NOT need to (and does not) COPY grader content -- only its dependencies.
FROM python:3.11-slim
RUN pip install --no-cache-dir flask==3.0.3 pytest==8.3.2
