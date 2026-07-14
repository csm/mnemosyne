# Task layer for sadserver-webstack: the agent container itself is the sick
# box. Build context is this fixture/ directory ONLY (run.sh enforces
# that); break.sh is generated host-side by prepare.sh/compose_break.py
# from a fault catalog kept in ../grader (never part of this context).
ARG BASE_IMAGE=mnemosyne-e2e-agent-base
FROM ${BASE_IMAGE}

USER root
RUN apt-get update && apt-get install -y --no-install-recommends \
    nginx python3 python3-pip sqlite3 procps util-linux \
    && rm -rf /var/lib/apt/lists/* \
    && pip install --break-system-packages --no-cache-dir flask==3.0.3

COPY app/app.py /opt/webstack/app.py
COPY webstack/app.env /etc/webstack/app.env
COPY nginx/app.conf /etc/nginx/conf.d/app.conf
COPY start.sh /usr/local/bin/start.sh
COPY start-app.sh /usr/local/bin/start-app.sh
COPY restart-services.sh /usr/local/bin/restart-services.sh
COPY logwriter.sh /usr/local/bin/logwriter.sh
COPY break.sh /usr/local/bin/break.sh
COPY prompt.md /task/prompt.md
RUN chmod +x /usr/local/bin/*.sh \
    && rm -f /etc/nginx/sites-enabled/default \
    && mkdir -p /data /var/log/webstack /task \
    && chown -R mnemosyne:mnemosyne /data /var/log/webstack /task /opt/webstack

# nginx and the app bind privileged-ish/system paths (/var/log/nginx,
# /run), so unlike the other tasks this container's entrypoint runs as
# root; the agent's shell access is still whatever clojure_eval /
# mnemosyne.shell grants, unchanged from the other scenarios.
EXPOSE 8080
WORKDIR /task
ENTRYPOINT ["/usr/local/bin/start.sh"]
