# Development acceptance fixture only. Not a production Runner installation.
FROM ubuntu@sha256:fe81560990fae86dc4743202b2cb4d3df98d0bcf949173cecef15ce1127ef852
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update -qq && apt-get install -y --no-install-recommends \
    python3.12=3.12.3-1ubuntu0.17 python3.12-venv=3.12.3-1ubuntu0.17 \
    openssh-server=1:9.6p1-3ubuntu13.20 bubblewrap=0.9.0-1ubuntu0.3 \
    ca-certificates=20260816ubuntu0.24.04.2 \
    && rm -f /etc/ssh/ssh_host_* && rm -rf /var/lib/apt/lists/* \
    && useradd --create-home --uid 10001 --user-group --shell /bin/sh runner \
    && usermod --password '' runner && mkdir -p /run/sshd
COPY environment.json /opt/consolecrypt/environment.json
COPY requirements-linux-x86_64-py312.lock /opt/consolecrypt/requirements-linux-x86_64-py312.lock
COPY test-fixture/install.py /opt/consolecrypt/install.py
RUN python3.12 -m venv /opt/cc-runtime && /opt/cc-runtime/bin/python /opt/consolecrypt/install.py
COPY src/ /opt/consolecrypt/src/
RUN cp /opt/consolecrypt/linux-x86_64-py312.json /opt/consolecrypt/src/consolecrypt_runner/environments/
COPY test-fixture/start.py /opt/consolecrypt/start.py
ENV PYTHONDONTWRITEBYTECODE=1
EXPOSE 2222
ENTRYPOINT ["/opt/cc-runtime/bin/python", "/opt/consolecrypt/start.py"]
