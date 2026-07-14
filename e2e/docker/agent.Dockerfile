# Base agent image: mnemosyne-mcp-server (semantic search optional)
# + Claude Code, the harness. Task images `FROM` this and layer in a fixture
# (see tasks/<id>/fixture/Dockerfile) — this file never sees a task's
# fixture or grader, so it is built once and reused across all tasks/seeds.
#
# Build context is the repo root (run.sh passes REPO_ROOT), since cargo
# needs the full workspace.
#
# Semantic search is disabled by default for e2e so CI does not depend on
# HuggingFace Hub availability or credentials. Set MNEMOSYNE_SEMANTIC=true to
# build with embeddings; that mode pre-fetches BAAI/bge-base-en-v1.5 while this
# stage still has internet egress, then bakes the HF cache into the runtime
# image for the sealed task network.

FROM rust:1-slim-bookworm AS build
WORKDIR /src
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev ca-certificates python3-pip && rm -rf /var/lib/apt/lists/* \
    && pip install --break-system-packages --no-cache-dir "huggingface_hub[cli]"
COPY . .
ARG MNEMOSYNE_SEMANTIC=false
RUN if [ "$MNEMOSYNE_SEMANTIC" = "true" ]; then \
      cargo build --release -p mnemosyne-mcp-server --features semantic; \
    else \
      cargo build --release -p mnemosyne-mcp-server --no-default-features; \
    fi

# Pre-fetch the embedding model only when semantic search is enabled. The Rust
# hf_hub crate and Python huggingface_hub CLI share the same on-disk cache
# layout under ~/.cache/huggingface/hub. Keep this repo id in sync with
# EmbedModel::default() in mnemosyne-semantic-search/src/embedder.rs.
RUN mkdir -p /root/.cache/huggingface \
    && if [ "$MNEMOSYNE_SEMANTIC" = "true" ]; then \
      huggingface-cli download BAAI/bge-base-en-v1.5 \
        tokenizer.json config.json model.safetensors; \
    fi

FROM node:20-bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl git && rm -rf /var/lib/apt/lists/* \
    && npm install -g @anthropic-ai/claude-code \
    && useradd --create-home --shell /bin/bash mnemosyne

COPY --from=build /src/target/release/mnemosyne-mcp-server /usr/local/bin/mnemosyne-mcp-server
COPY --from=build /root/.cache/huggingface /home/mnemosyne/.cache/huggingface

RUN mkdir -p /mnemosyne-data /task /results \
    && chown -R mnemosyne:mnemosyne /mnemosyne-data /task /results /home/mnemosyne/.cache

USER mnemosyne
ENV HOME=/home/mnemosyne
WORKDIR /task

# No ENTRYPOINT/CMD: run.sh always supplies the full harness command
# (`timeout ... bash -c "claude -p ..."`) as the `docker run` argv.
