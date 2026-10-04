# OCI version/layer reuse acceptance

- Candidate: 6f0f31a06f1833dcdadce9525161cb5e9f55a779
- Source date epoch: 0
- Synthetic package versions: 0.0.0-ci.1 and 0.0.0-ci.2
- Platforms: linux/amd64
- Scenario: same source and SemVer with different Docker-context mtimes; then same source with different SemVer
- Note: SemVer A/B is a synthetic packaging contract test, not evidence of a historical production version-only release.

## Same-version mtime comparison

| Platform    | Filesystem layers | Same-version mtime comparison | SemVer layer changes                       |
| ----------- | ----------------: | ----------------------------- | ------------------------------------------ |
| linux/amd64 |                21 | all RootFS diffIDs match      | exactly main binary + frontend application |

## Same-version RootFS diffIDs

| Platform    | Index | RootFS diffID                                                             |
| ----------- | ----: | ------------------------------------------------------------------------- |
| linux/amd64 |     0 | `sha256:66462cc862fe2053b9863fefa3866e07bb5dfb06f6b3ce3177cc096e4021aabe` |
| linux/amd64 |     1 | `sha256:931f3939cb53e94bb2d1b5c016ed093d37694c3af0ef421eff074ecfa2e38521` |
| linux/amd64 |     2 | `sha256:8bb9c5dff6a38a5e35bbbbfaaf74092b7662028e19966e97030f0fc702c29400` |
| linux/amd64 |     3 | `sha256:2f5bc0ebb6459de970ab2dbf6e622cba2052c4a2e8c018dbda8a545c56ab6063` |
| linux/amd64 |     4 | `sha256:7df2767c4409503637d19848d24b1da53602025a69b4e2c68119d714ae8746c6` |
| linux/amd64 |     5 | `sha256:076b869c2c43e9d673f65193bbaa37cc446304c68e4454e32715464943289424` |
| linux/amd64 |     6 | `sha256:bc3216104935f82fd4029712e93e667d0f9fb492ca9ac7021709dd6632811de2` |
| linux/amd64 |     7 | `sha256:11a6fb87e09daf3f2c7a7981dc9907fd514f8c9d8e2bdfa61932eff96a839633` |
| linux/amd64 |     8 | `sha256:48c68fff5dbf4007e0d7be9af321f52e54c840d7749b56c6315d6167fa152762` |
| linux/amd64 |     9 | `sha256:9132ae9dd7615067523c2ede6b99a1f7697490415932e72e4fa6ed1db972ff1b` |
| linux/amd64 |    10 | `sha256:b788dbe7705a5153e5fd9e021f892ad55a02ebe7eb861e28cbe4a6fd8182e940` |
| linux/amd64 |    11 | `sha256:d3da18570df95692ac3d1d7f5d795a99021f6beb1f667450fab43d6ce72479a7` |
| linux/amd64 |    12 | `sha256:1d643af5d7180e717fb844a02df6fec279a16d1d861868bb686c1a086fd9b00f` |
| linux/amd64 |    13 | `sha256:a4ad9b650e4c3857f1171603b34d3530ca770fc49db0f1738854e6843b63db01` |
| linux/amd64 |    14 | `sha256:2a68f32cd2e696b00653b5622b2913cab5ef84d2c03870ccefa00d9ffc94ffcc` |
| linux/amd64 |    15 | `sha256:3d285f52633cd77e3f56063230a7d4248bc0e5146caa693dec99ca165476b175` |
| linux/amd64 |    16 | `sha256:1f1a52883693c7a70b10ec1ff3f3eba4c9a149993f293b4663966386f01a8263` |
| linux/amd64 |    17 | `sha256:3e23d16bf43d0611f56457145815891f7d5abc2811ab8ae24642d9123d94137e` |
| linux/amd64 |    18 | `sha256:ed7eff9072d892ea5fa9296bc7407718001887f8feb477920870d1cb53f1bf85` |
| linux/amd64 |    19 | `sha256:da43314bf67e114e3aaa85f711563a48a5ae106195b6cdc3e6d4972b76f63421` |
| linux/amd64 |    20 | `sha256:52c11d4fc05f7bc376631d3bc7c76048fc46fd09c075318c4a168af30234ee95` |

## Changed layer compressed byte estimates

| Platform    | RootFS diffID index | Layer payload        | Compressed bytes | Method                           |
| ----------- | ------------------: | -------------------- | ---------------: | -------------------------------- |
| linux/amd64 |                   5 | main-service-binary  |         15498527 | gzip -1 of docker-save layer.tar |
| linux/amd64 |                  18 | frontend-application |          7431177 | gzip -1 of docker-save layer.tar |
