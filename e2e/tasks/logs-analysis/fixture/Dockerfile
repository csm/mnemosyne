# Task layer for logs-analysis. Build context is this fixture/ directory
# ONLY (run.sh enforces that). `logs/` is generated on the HOST by
# prepare.sh/generate.py before this build runs (see run.sh) -- ground
# truth (answers.json) never touches this directory or this build context.
ARG BASE_IMAGE=mnemosyne-e2e-agent-base
FROM ${BASE_IMAGE}

COPY logs /task/logs
COPY prompt.md /task/prompt.md
USER root
RUN chown -R mnemosyne:mnemosyne /task
USER mnemosyne
WORKDIR /task
