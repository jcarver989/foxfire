# syntax=docker/dockerfile:1.7
#
# A small stand-in for the real image: the same entrypoint and workspace
# layout, with a relay that only idles and no Cargo build.

FROM alpine:3.22
RUN apk add --no-cache git \
    && adduser -D dev \
    && install -d -o dev -g dev /workspace
COPY --chmod=755 entrypoint.sh /usr/local/bin/foxfire-entrypoint
COPY --chmod=755 <<'RELAY' /usr/local/bin/foxfire-relay
#!/bin/sh
exec tail -f /dev/null
RELAY
USER dev
ARG SOURCE_SHA
WORKDIR /workspace
RUN --mount=type=bind,from=project-src,target=/tmp/project-src \
    git init --quiet \
    && git bundle unbundle /tmp/project-src/repo.bundle > /dev/null \
    && git -c advice.detachedHead=false checkout --quiet --detach "${SOURCE_SHA}"
EXPOSE 7777
ENTRYPOINT ["foxfire-entrypoint"]
