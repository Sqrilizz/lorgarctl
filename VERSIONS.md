# Component versions

Single source of truth for release version drift. CI (`tools/check-versions.sh`)
fails when these diverge.

| Component | Version | Source |
| :--- | :---: | :--- |
| Rust workspace (`lorgar-core`, `lorgarctl`, `lorgard`) | 0.1.0 | `Cargo.toml` `[workspace.package] version` |
| Kernel module (`hid-lorgar`) | 2.3.4 | `kernel/dkms.conf` `PACKAGE_VERSION` + `kernel/hid-lorgar.c` `MODULE_VERSION` |
| Plasmoid widget | 1.0 | `plasmoid/org.lorgar.battery/metadata.json` |
| Release tag | `v2.x.y` | git tag (kernel-lineage version, not Rust version) |

Rules:
- `kernel/dkms.conf` `PACKAGE_VERSION` must equal `MODULE_VERSION` in `kernel/hid-lorgar.c`.
- `make -C kernel check` enforces the kernel version match against the built `.ko`.
- Rust workspace version and kernel version are intentionally independent lines;
  do not force them equal, only keep this file in sync.
