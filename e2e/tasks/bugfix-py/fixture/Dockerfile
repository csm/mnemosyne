# Task layer for bugfix-py. FROM's the shared agent base image; build
# context is this fixture/ directory ONLY (run.sh enforces that), so there
# is no path by which a COPY here could reach ../grader.
ARG BASE_IMAGE=mnemosyne-e2e-agent-base
FROM ${BASE_IMAGE}

USER root
RUN apt-get update && apt-get install -y --no-install-recommends python3 python3-pip \
    && rm -rf /var/lib/apt/lists/*

COPY project /task/project
COPY prompt.md /task/prompt.md
RUN chown -R mnemosyne:mnemosyne /task \
    && pip install --break-system-packages --no-cache-dir -r /task/project/requirements.txt

USER mnemosyne
WORKDIR /task
