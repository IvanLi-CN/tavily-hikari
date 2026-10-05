# OCI version/layer reuse acceptance

- Candidate: 0afdb307e4597e4c131bc4cb1848e6ba2d1ae522
- Source date epoch: 0
- Synthetic package versions: 0.0.0-ci.1 and 0.0.0-ci.2
- Platforms: linux/amd64
- ARM64 evidence: not tested; this report provides no ARM64 evidence
- Scenario: same source and SemVer with different Docker-context mtimes; then same source with different SemVer
- Note: SemVer A/B is a synthetic packaging contract test, not evidence of a historical production version-only release.
- Raw build log: [oci-acceptance-amd64-0afdb307.log](oci-acceptance-amd64-0afdb307.log)
- Raw build log SHA-256: `5563ed0ac8db0ca23ad4a43f360a44897be98289930e746d2cf37e83b0140086`

## Same-version mtime comparison

| Platform    | Filesystem layers | Same-version mtime comparison | SemVer layer changes                       |
| ----------- | ----------------: | ----------------------------- | ------------------------------------------ |
| linux/amd64 |                21 | all RootFS diffIDs match      | exactly main binary + frontend application |

## Same-version RootFS diffIDs

| Platform    | Index | RootFS diffID                                                             |
| ----------- | ----: | ------------------------------------------------------------------------- |
| linux/amd64 |     0 | `sha256:66462cc862fe2053b9863fefa3866e07bb5dfb06f6b3ce3177cc096e4021aabe` |
| linux/amd64 |     1 | `sha256:ad44e4801ec7c6fcc9c92396cea2e7089ddc9e8c3fe75c07933ccac31f1d4296` |
| linux/amd64 |     2 | `sha256:4cf8b1708211710c1c8eadc1ffd495c29cd20bf5510c0abb19d870371a3fcde5` |
| linux/amd64 |     3 | `sha256:76201b669d96c96e4f778cdaefa3c6395cbc6d5788f75e33f26452ac500c2402` |
| linux/amd64 |     4 | `sha256:a6ce2abab06f870b8d2a3d89f5776061835d5e421cc7efb78b44df99d311ef67` |
| linux/amd64 |     5 | `sha256:227b02d4d7e991e433351dbc4700f4bd4bd51645dbcb6a41df8c43e2fed7867e` |
| linux/amd64 |     6 | `sha256:3577843f264344507a073afa6a34e0e70148de73db4b5978fe70c031a8871ee3` |
| linux/amd64 |     7 | `sha256:e7b4b0558b088de7fedc0bc7b92d34011f971786697cddcc3edbd403d6f0c449` |
| linux/amd64 |     8 | `sha256:57e001aeb842227f20d139689a2790c30e49335642cdef6b426275f45960acfb` |
| linux/amd64 |     9 | `sha256:38b241ffeab2dd3a7393624cf41239848c527c9498d8351e726351569942977b` |
| linux/amd64 |    10 | `sha256:d06007caeb022430ae3fb81030b900f1c10e1219abf230eefa9e44dbd37c903a` |
| linux/amd64 |    11 | `sha256:de58fc0b7adca7e0d3cbc07cfbe815c93c965e615bfc5ae23d63fed5a24c654e` |
| linux/amd64 |    12 | `sha256:a47e8be5fc99e5a6e94b6b51b4f6f855f39d8cc05493eeaca6d79856a350d7b4` |
| linux/amd64 |    13 | `sha256:0d2aa911dc73c31053d508ddb40767ab6b2c1f677c906a926631e95b06c901cc` |
| linux/amd64 |    14 | `sha256:35588404df89532ac84f39a6dd785f9597f94946bc824e5a3b886cda3870e326` |
| linux/amd64 |    15 | `sha256:01a7175e7d43e08d846778b75f5bfa80d08741b7e368e8d2eae0e162e65edba8` |
| linux/amd64 |    16 | `sha256:75a79ceecaeb89bf6963a1010e2810d6ae01d8589966e1827970f81285923bf6` |
| linux/amd64 |    17 | `sha256:94c46449f0f31ec28cd7f2c2be167ad04379234e7bfae0a8e1618bce9cbec904` |
| linux/amd64 |    18 | `sha256:6159cffe295337c03265f81fc4aeb05c104812ee5b89b2ed814dfb8ddde074f0` |
| linux/amd64 |    19 | `sha256:3478b4fdd51bfcbfa413c3f716fd9486257d544a9f3dab875c16015b488ae411` |
| linux/amd64 |    20 | `sha256:24db42a51a2d214e16c17fb917fb6ae0f15d0eedd7534514f0b446bad0c75c91` |

## Changed layer compressed byte estimates

| Platform    | RootFS diffID index | Layer payload        | Compressed bytes | Method                           |
| ----------- | ------------------: | -------------------- | ---------------: | -------------------------------- |
| linux/amd64 |                   5 | main-service-binary  |         16926216 | gzip -1 of docker-save layer.tar |
| linux/amd64 |                  18 | frontend-application |          7734948 | gzip -1 of docker-save layer.tar |

## Packaged CLI versions

- Image `0.0.0-ci.1`: `--version` reports `tavily-hikari 0.0.0-ci.1`.
- Image `0.0.0-ci.2`: `--version` reports `tavily-hikari 0.0.0-ci.2`.

## Runtime and OCI version checks

- Image `0.0.0-ci.1`:
  - `--version`: `tavily-hikari 0.0.0-ci.1`
  - `/api/version`: `{"backend":"0.0.0-ci.1","frontend":"0.0.0-ci.1"}`
  - `/version.json`: `{"version":"0.0.0-ci.1"}`
  - OCI `org.opencontainers.image.version`: `0.0.0-ci.1`
  - `APP_EFFECTIVE_VERSION` runtime environment: absent
  - Static `/srv/app/web/version.json`: absent
- Image `0.0.0-ci.2`:
  - `--version`: `tavily-hikari 0.0.0-ci.2`
  - `/api/version`: `{"backend":"0.0.0-ci.2","frontend":"0.0.0-ci.2"}`
  - `/version.json`: `{"version":"0.0.0-ci.2"}`
  - OCI `org.opencontainers.image.version`: `0.0.0-ci.2`
  - `APP_EFFECTIVE_VERSION` runtime environment: absent
  - Static `/srv/app/web/version.json`: absent
