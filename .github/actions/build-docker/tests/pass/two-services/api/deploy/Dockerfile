# The service api: it marks itself ready in its volume, as the user nobody,
# and its health check looks for the mark.
FROM busybox:1.37.0-musl@sha256:f6a95d611e0a8780fabdb2aec43e31275b2720dfefa3e41b16835bfc42454478
RUN mkdir /data && chown 65534:65534 /data
USER 65534:65534
VOLUME /data
HEALTHCHECK --interval=1s --timeout=1s --retries=3 CMD ["test", "-f", "/data/ready"]
CMD ["sh", "-c", "echo api starts && touch /data/ready && exec sleep 3600"]
