# Base agent image: mnemosyne-mcp-server (built with the `semantic` feature)
# + Claude Code and Codex CLI, the harnesses. Task images `FROM` this and layer in a fixture
# (see tasks/<id>/fixture/Dockerfile) — this file never sees a task's
# fixture or grader, so it is built once and reused across all tasks/seeds.
#
# Build context is the repo root (run.sh passes REPO_ROOT), since cargo
# needs the full workspace.
#
# The `semantic` feature's first `function_lookup` call downloads a BERT
# model (BAAI/bge-base-en-v1.5, see mnemosyne-semantic-search/src/embedder.rs)
# from HuggingFace Hub. The sealed task network (`tasknet`, internal) has no
# route there, so it is pre-fetched here at build time, while this stage
# still has normal internet egress, and baked into the image's HF cache.

FROM rust:1-slim-bookworm AS build
WORKDIR /src
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev ca-certificates python3-pip && rm -rf /var/lib/apt/lists/* \
    && pip install --break-system-packages --no-cache-dir "huggingface_hub[cli]"
COPY . .
# `semantic` is mnemosyne-mcp-server's default feature (see its Cargo.toml);
# built explicitly here so the Dockerfile stays correct if that changes.
RUN cargo build --release -p mnemosyne-mcp-server --features semantic

# Pre-fetch the embedding model into this stage's HF cache (the Rust hf_hub
# crate and the Python huggingface_hub CLI share the same on-disk cache
# layout under ~/.cache/huggingface/hub) so it can be copied verbatim into
# the runtime image and the sealed task network never needs to reach
# HuggingFace. Keep this repo id in sync with EmbedModel::default() in
# mnemosyne-semantic-search/src/embedder.rs.
RUN huggingface-cli download BAAI/bge-base-en-v1.5 \
    tokenizer.json config.json model.safetensors

FROM node:20-bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl git && rm -rf /var/lib/apt/lists/* \
    && npm install -g @anthropic-ai/claude-code @openai/codex \
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
