# Task layer for bugfix-clj. FROM's the shared agent base image; build
# context is this fixture/ directory ONLY (run.sh enforces that), so there
# is no path by which a COPY here could reach ../grader.
ARG BASE_IMAGE=mnemosyne-e2e-agent-base
FROM ${BASE_IMAGE}

COPY project /task/project
COPY prompt.md /task/prompt.md
USER root
RUN chown -R mnemosyne:mnemosyne /task
USER mnemosyne
WORKDIR /task
