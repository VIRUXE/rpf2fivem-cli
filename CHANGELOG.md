# Changelog

## 0.1.7 - 2026-06-07

### Changed

- Use a strict FiveM-safe allowlist during conversion.
- Copy data files only when they map to a known FiveM `data_file` mounter.
- Preserve semantic data paths to prevent same-name file collisions.
- Support additional known streamed asset formats and organize maps and paths.
- Generate path-aware manifests with explicit data-file directives.
- Document the `rpf-rs`/`rpf-archive` backend and conversion safety model.

### Safety

- Omit scenario, scenario-info, and single-player manifest data.
- Omit unsupported dispatch, combat, movement, relationship, and ped-health data.
- Omit unknown `.meta`, `.ymt`, `.dat`, `.xml`, and streamed file formats.
- Prevent unknown `.ymt` and `.ipl` files from entering generated resources.
