# ADR 0007: Release Version Embedding and Image Layer Reuse

Status: Accepted

## Context

Production image comparisons showed that copied files with identical paths and bytes could still produce different OCI layer digests when their filesystem mtimes changed. A separate static `version.json` also created a metadata-only image layer. Product version data currently has multiple runtime sources: an environment variable, generated HTML metadata, a static JSON asset, and service-worker build metadata.

Inspection of exported OCI layers showed two separate sources of unnecessary SemVer-dependent layers. Runtime `RUN --mount` steps recorded build-time mtimes for `/etc` and `/tmp` in each single-file install layer. Later recursive `find ... touch` commands walked the whole web root again, so PWA and metadata layers rewrote files owned by the versioned application layer, including the changing asset graph. Both effects changed otherwise reusable layer diffIDs.

The release workflow already resolves one product SemVer and shares a single `web/dist` artifact with Docker and native binary jobs. The version must remain visible through the existing `/api/version` and `/version.json` contracts and must continue to support an explicit external static directory override.

## Decision

- Embed the resolved product SemVer into the `tavily-hikari` server binary at compile time and into the frontend JavaScript application package at Vite build time. The server's CLI `--version` and version API use that same embedded value. Use the same release version input for web assets, native Linux packages, and Docker builds.
- Use that SemVer in both public and admin service-worker cache identities. Read the current frontend version from the running JavaScript application package rather than an HTML meta tag or standalone metadata file.
- Keep `/api/version` response fields and `/version.json` JSON shape compatible. Generate `/version.json` dynamically from the packaged version; when an explicitly configured external static directory supplies `version.json`, preserve its version override behavior.
- Do not ship a static `version.json` in production `web/dist` or Docker images, and do not create a filesystem layer whose only payload is release-version metadata. Keep the OCI version label as image metadata and do not expose `APP_EFFECTIVE_VERSION` as a runtime environment variable.
- Normalize files and directories in the payload stage to `SOURCE_DATE_EPOCH=0`, owner `0:0`, directories and ordinary files to mode `0755` and `0644`, and executable binaries/scripts to `0755`. Write each payload into its final layer from a read-only BuildKit bind mount, then reset changed destination directories plus `/etc` and `/tmp` in that same layer. Later PWA and metadata layers copy only their own payload and reset affected directories; they must not walk or restamp files already owned by the application layer.
- Keep each maintenance executable in its own layer and compile those ten utilities without release SemVer. Compile the main service binary separately with the SemVer build input. Put `/assets`, HTML shells, and both service workers in one frontend application layer that contains the actual versioned JavaScript; keep PWA icons/manifests and runtime scripts in separate normalized layers.
- Require Docker-enabled Linux VM evidence on `linux/amd64`: same-source/same-version builds with varied input mtimes have matching filesystem layer digests; a same-source synthetic SemVer A/B changes only the main server binary and frontend application package layers. Record compressed bytes for changed layers. This synthetic packaging result is not evidence that a production release historically changed only its version number.

## Consequences

- Backend API, CLI, and frontend report the same product release SemVer by default; external static `version.json` remains an explicit frontend version override.
- A release changes the frontend application package layer because its JavaScript bundle embeds the SemVer. Its compressed download size is measured; it is not constrained to the size of the former metadata layer.
- A release changes the main service binary layer and OCI image metadata. The ten maintenance executable layers and normalized static layers remain reusable when their content is unchanged.
- Deterministic file and destination-directory mtime, owner, and mode normalization makes layer reuse independent of source checkout timestamps while retaining executable permissions.
- The HTTP `/version.json` compatibility route remains available without placing a static JSON file in release artifacts or the production image.

## Alternatives Rejected

- Keep the SemVer only in a static `version.json` layer: rejected because it preserves a metadata-only filesystem layer and leaves frontend identity outside the actual application package.
- Use a frontend-only build ID: rejected because the product requirement is that the frontend, backend, native package, Docker image, and release workflow report the same product SemVer.
- Remove `/version.json`: rejected because existing clients and deployment checks rely on its JSON response contract.
- Add a timestamp-fixing overlay after each final-image COPY: rejected because the COPY layer itself retains the changing parent-directory mtime and still needs to be downloaded. The file write and directory timestamp reset must belong to the same layer.
