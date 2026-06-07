# rpf2fivem

Convert GTA V mod archives into FiveM resource folders.

`rpf2fivem` extracts ZIP, RAR, and 7z packages, reads embedded RPF7 archives
with the [`rpf-rs`](https://github.com/VIRUXE/rpf-rs) project's
`rpf-archive` crate, and generates a FiveM resource with an `fxmanifest.lua`.

## Safety Model

Conversion is intentionally not lossless. GTA V archives can contain files
which FiveM cannot mount or stream safely. Including those files in a generated
resource can cause resource startup failures or client crashes.

The converter therefore uses a strict allowlist:

- A data file is copied only when it maps to a known FiveM `data_file` type.
- A streamed asset is copied only when its extension is known to be supported.
- Unknown and unsupported files are omitted from the generated resource.
- Generated manifests contain explicit paths and known mounter types.

This means a successful conversion is a FiveM-compatible subset of the input,
not a complete extraction of every file in the archive.

## Supported Streamed Assets

The following formats are copied into `stream/`:

```text
.yft .ytd .ydr .ydd .ybn .ymap .ytyp .ynd .ynv .ycd
```

Map assets are placed under `stream/maps/`. Path and navigation assets are
placed under `stream/paths/`.

## Supported Data Files

Known data files are copied and registered with explicit `data_file`
directives. Supported types include:

```text
HANDLING_FILE
VEHICLE_METADATA_FILE
VEHICLE_LAYOUTS_FILE
CARCOLS_FILE
VEHICLE_VARIATION_FILE
DLCTEXT_FILE
CARCONTENTUNLOCKS_FILE
PED_METADATA_FILE
PED_PERSONALITY_FILE
DLC_POP_GROUPS
POPSCHED_FILE
ZONEBIND_FILE
AMBIENT_PED_MODEL_SET_FILE
AMBIENT_VEHICLE_MODEL_SET_FILE
AMBIENT_PROP_MODEL_SET_FILE
CONDITIONAL_ANIMS_FILE
CLIP_SETS_FILE
LOADOUTS_FILE
PED_TASK_DATA_FILE
PED_COMPONENT_SETS_FILE
PED_PERCEPTION_FILE
```

Semantic data paths are preserved. Files such as `vehicles/main.meta` and
`peds/main.meta` remain separate instead of overwriting each other.

Audio wavepacks and matched game/sound configuration files are also supported.

## Omitted Files

The converter omits files that do not have a known safe FiveM conversion path,
including:

- Scenario, scenario-info, and single-player manifest data
- Unsupported dispatch, combat, movement, relationship, and ped-health data
- Unknown `.meta`, `.ymt`, `.dat`, and `.xml` files
- Unknown streamed formats

Omitted files are not copied into the output resource and are not added to the
manifest.

## Usage

```text
rpf2fivem [OPTIONS] [INPUT]
```

Convert a local archive:

```powershell
rpf2fivem mod.zip --output resources
```

Convert a direct download or GTA5-Mods page:

```powershell
rpf2fivem https://www.gta5-mods.com/vehicles/example --output resources
```

Choose a resource name and overwrite an existing output folder:

```powershell
rpf2fivem mod.7z --name example_car --output resources --yes
```

Run `rpf2fivem --help` for all options.

## Build

Requires the Rust toolchain.

```powershell
cargo test
cargo build --release
```

The release binary is written to `target/release/rpf2fivem.exe`.

Tagged GitHub releases provide native archives for:

- Windows x86-64
- Linux x86-64

Pushing a `v*` tag builds, tests, packages, and uploads both binaries through
GitHub Actions. The release workflow can also be dispatched manually to rebuild
assets for an existing tag.

## License

Unlicense
