# SSH test container for ConsoleCrypt integration tests (CLIENT_SPEC §19).
# Nothing secret is baked in: host keys, user password, authorized keys and
# CA keys are provided/generated at container start.
FROM alpine:3.20
RUN apk add --no-cache openssh-server openssh-keygen busybox-extras bash \
 && adduser -D -s /bin/sh tester \
 && mkdir -p /home/tester/.ssh /srv/www \
 && chmod 700 /home/tester/.ssh \
 && chown -R tester:tester /home/tester
COPY sshd_config /etc/ssh/sshd_config
COPY entrypoint.sh /entrypoint.sh
RUN chmod 0755 /entrypoint.sh
EXPOSE 22
ENTRYPOINT ["/entrypoint.sh"]
