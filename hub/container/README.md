# The hub container

Open code, self-hostable. The same static hub binary runs in hosting and self-hosting. `Dockerfile` copies the
checked binary into a pinned distroless image: non-root, port 8080, a real Docker healthcheck
(`trommi-hub healthcheck`), no shell or Docker socket. Use a writable data mount, read-only configuration and
root filesystem, and no published host ports when your proxy shares the private network.

## Hosting and signatures

Our server runs `/home/docker/trommi/compose.yaml`: alias `trommi` in the external network `web`, data at
`/home/docker/trommi/data`, configuration at `/home/docker/trommi/config`. Cloudflare Tunnel sends
`hub.trommi.com` to `http://trommi:8080`. The old systemd hub/updater are disabled. `/srv/trommi/data` is the old
installation, not live data. The example's `995:983` preserves our existing ownership; self-hosters choose their
own uid/gid. Raw environment files preserve dollar signs in encoded password hashes.

There are two separate signing keys:

- `release/public-key.pem`: the existing Ed25519 key for binary/connector release manifests; private key in
  1Password. These signed releases remain available. The hub's signed manifest also carries the public container key.
- `release/hub-cosign-public.pem`: a standard Cosign ECDSA P-256 key for OCI images. Its public copy is pinned at
  `/home/docker/yolodeploy/keys/trommi.pub`. The private key stays in 1Password as `SIGN_IMAGE_KEY`
  (unencrypted PKCS8 PEM); `SIGN_IMAGE_PUBLIC_KEY` holds its public counterpart. CI imports the private key
  with a random runtime password and deletes the ephemeral files afterwards. No private release/image key
  is installed on the server. Use unmodified Cosign 3.1.3 with its transparency verification.

To rotate the image key, import the replacement into 1Password and update the reviewed repository public pin
and server public pin together. A mismatched key fails before promotion. Never paste a private key into a log or PR.

## Checked build to deployment

1. `main` runs shared and hub checks, builds static binaries and tests the isolated non-root container's actual
   Docker healthcheck and exact commit. It checks that the running and latest released predecessors still start
   on data this build migrated. Raising `hub/data-epoch` no longer skips this check: migrations must expand first
   and remain compatible with the previous image. Unknown old schemas remain refused rather than guessed at.
2. Producing-job conclusions and artifact digests are verified before Ed25519 signing and publishing `hub-v<N>`.
   The delivery job rechecks the manifest, signature and asset bytes before building the image. No built hub runs
   in a signing job. Buildx's own digest output, not a mutable registry tag lookup, selects what Cosign signs.
3. CI publishes `ghcr.io/chriopter/trommi-hub:hub-v<N>`, signs/verifies its immutable digest, and only then promotes
   `production`, provided no newer release exists. The promotion copies the same manifest, with no index wrapping.
   Per-hub serialization protects promotion and deployment. The signed `io.yolodeploy.sequence` label prevents
   normal image downgrades or replacing an accepted sequence with different app bytes.
4. CI checks the deploy route separately, then sends one empty POST to `DEPLOY_WEBHOOK_URL` with
   `CF-Access-Client-Id`, `CF-Access-Client-Secret`, `Authorization: Bearer <DEPLOY_TOKEN>` and
   `User-Agent: Trommi-Deploy/1.0`. The explicit User-Agent fixes the previous Cloudflare error 1010.
   Credentials never follow redirects or appear in output. A POST 401 is the app-token layer; an edge 403 is
   checked before an update. Ambiguous deployment failures are not blindly retried.
5. The token selects YoloDeploy's fixed Trommi target. It pulls and verifies Cosign before replacing the app,
   starts the verified immutable image ID, and waits for Docker health. Failure starts the previous image on the
   **current data**. It copies, archives and resets no app data. CI additionally requires public `/healthz` to
   report `ok`, a protocol version and exactly this commit. Re-run the checked `main` hub delivery for recovery.

The target's recovery block has only `directory: /home/docker/yolodeploy/recovery/trommi` and
`signature_key: /home/docker/yolodeploy/keys/trommi.pub`. No `data`, `sqlite` or `restore_sqlite` fields.
The directory is operator-owned, mode 0700 and writable in YoloDeploy. Compose, configuration and public keys
are mounted read-only at identical host/container paths. Other fixed targets stay unchanged. GHCR images and
signature material must be readable by the server; an unavailable registry/verifier stops the update beforehand.

## Secret settings

The hub's secrets come from `/home/docker/trommi/config/hub-secrets.env` (root only, read by Docker when it creates
the container), not from 1Password directly. Today they are the push credentials and `HUB_TINFOIL_API_KEY`, the
Tinfoil key of sealed dictation (spec/v1.md 18; without it the hub offers no dictation). The key lives in the hub's
1Password Environment; `hub/container/secrets.sh` puts it on the server and recreates the container, the value on
standard input and never printed:

```bash
op run --environment <hub production environment id> -- hub/container/secrets.sh
```

## Regular backups and rollback

Image rollback preserves writes, so database changes must stay compatible with the previous image. Schema 4
and schema 5 are distinct layouts; accepting an unknown version would not make them compatible. The signed
predecessor-binary migration test is the deployment gate, including across declared epoch changes.

`backup.py` is a separate nightly systemd job. It holds YoloDeploy's private `operation.lock`, briefly pauses the
hub for a consistent copy of its whole data/configuration, then resumes it and waits for Docker health before
releasing the lock. Compression runs afterwards. The copied SQLite DB/WAL is normalized with SQLite's backup
API and checked; the resulting private archive is read back, checksummed and synced. An interruption marker and
`ExecStopPost --resume` resume only the container this job paused. Another operator's paused container is left
alone. Copy failure publishes no archive.

The timer runs at 03:10 Europe/Berlin. Archives under `/srv/trommi/backups/container` are private (directory 0700,
files 0600), retained 14 days; old migration backups and deployment history are untouched. Restore testing uses
isolated data. Before manual restoration, stop the hub, verify the checksum, and select a compatible image.
After a power loss or killed deployment, inspect the recovery journal manually. Do not re-enable systemd over
its stale data. Local backups do not replace an off-server copy; the owner still needs to arrange that.

## Self-hosting and limits

Adapt `compose.yaml` with your paths, non-root ownership, canonical `HUB_URL`, origins and push settings. Join
an external network shared with your reverse proxy, or adapt the private network to your proxy. Keep a real
Docker healthcheck and no Docker socket in the hub. YoloDeploy needs only a fixed target, private app token,
private writable recovery directory and independently pinned public key. Back up data separately.

YoloDeploy's Docker socket grants broad host rights. The CF service credentials are shared across apps; the
app tokens are separate. UFW is inactive. The owner should rotate the tunnel token and the Trommi deploy token
shown during setup. No off-server backup is configured. New app links and release feeds use
`github.com/chriopter/trommi`. Old connector 2003/1048 binaries reject GitHub's repository redirect: after the new
connector is published, run the current signed installer once. Historical signed manifest identities remain
accepted only for the known transferred repository; arbitrary repositories are refused.
