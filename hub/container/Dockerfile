# The binary is built, tested, signed and checked before this context is assembled. No app runs while signing.
FROM gcr.io/distroless/static-debian12:nonroot@sha256:afa5c872c891853ca7fcf1f12c3edb23f7eeef36189728842dd51042ff57f7ab
COPY --chmod=0555 trommi-hub /usr/local/bin/trommi-hub
ENV HUB_HOST=0.0.0.0 HUB_PORT=8080 HUB_DATA=/data
EXPOSE 8080
USER 65532:65532
HEALTHCHECK --interval=10s --timeout=4s --start-period=10s --retries=3 CMD ["/usr/local/bin/trommi-hub", "healthcheck"]
ENTRYPOINT ["/usr/local/bin/trommi-hub"]
