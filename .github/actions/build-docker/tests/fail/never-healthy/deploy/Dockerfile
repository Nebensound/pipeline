FROM busybox:1.37.0-musl@sha256:f6a95d611e0a8780fabdb2aec43e31275b2720dfefa3e41b16835bfc42454478
USER 65534:65534
HEALTHCHECK --interval=1s --timeout=1s --retries=2 CMD ["sh", "-c", "echo not ready yet; exit 1"]
CMD ["sh", "-c", "echo the service starts && exec sleep 3600"]
