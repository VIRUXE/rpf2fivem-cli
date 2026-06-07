use std::collections::BTreeSet;
use std::path::Path;

/// A data file written into the resource and, when supported, its FiveM mounter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DataFile {
    /// Path relative to the resource root, using `/`.
    pub path: String,
    pub directive: &'static str,
}

/// Infer the FiveM data_file type from a resource-relative data path.
pub fn directive_for(path: &str) -> Option<&'static str> {
    let lower = path.replace('\\', "/").to_ascii_lowercase();
    let basename = Path::new(&lower).file_name()?.to_str()?;
    let extension = Path::new(basename).extension()?.to_str()?;

    if extension == "meta" && (lower.contains("/handling/") || basename == "handling.meta") {
        Some("HANDLING_FILE")
    } else if extension == "meta"
        && (lower.contains("/vehiclelayouts/") || basename == "vehiclelayouts.meta")
    {
        Some("VEHICLE_LAYOUTS_FILE")
    } else if extension == "meta" && (lower.contains("/vehicles/") || basename == "vehicles.meta") {
        Some("VEHICLE_METADATA_FILE")
    } else if extension == "meta" && (lower.contains("/peds/") || basename == "peds.meta") {
        Some("PED_METADATA_FILE")
    } else if matches!(extension, "ymt" | "xml") && lower.contains("/clipsets/") {
        Some("CLIP_SETS_FILE")
    } else {
        match basename {
            "carcols.meta" => Some("CARCOLS_FILE"),
            "carvariations.meta" => Some("VEHICLE_VARIATION_FILE"),
            "dlctext.meta" => Some("DLCTEXT_FILE"),
            "contentunlocks.meta" => Some("CARCONTENTUNLOCKS_FILE"),
            "vehiclemodelsets.meta" => Some("AMBIENT_VEHICLE_MODEL_SET_FILE"),
            "ambientpedmodelsets.meta" => Some("AMBIENT_PED_MODEL_SET_FILE"),
            "propsets.meta" => Some("AMBIENT_PROP_MODEL_SET_FILE"),
            "conditionalanims.meta" => Some("CONDITIONAL_ANIMS_FILE"),
            "loadouts.meta" => Some("LOADOUTS_FILE"),
            "taskdata.meta" => Some("PED_TASK_DATA_FILE"),
            "pedcomponentsets.meta" => Some("PED_COMPONENT_SETS_FILE"),
            "pedperception.meta" => Some("PED_PERCEPTION_FILE"),
            "pedpersonality.ymt" => Some("PED_PERSONALITY_FILE"),
            "popgroups.ymt" => Some("DLC_POP_GROUPS"),
            "popcycle.dat" => Some("POPSCHED_FILE"),
            "zonebind.ymt" => Some("ZONEBIND_FILE"),
            _ => None,
        }
    }
}

/// Audio layout discovered after extraction (paths use `/` and are relative to the resource root).
pub struct AudioManifest {
    pub wavepacks: Vec<String>,
    pub physical_files: Vec<String>,
    pub game_sound_data: Vec<(String, String)>,
}

impl AudioManifest {
    pub fn is_empty(&self) -> bool {
        self.wavepacks.is_empty() && self.game_sound_data.is_empty()
    }
}

pub fn single(
    data_files: &[DataFile],
    audio: &AudioManifest,
    description: Option<&str>,
    url: Option<&str>,
) -> String {
    generate(data_files, audio, description, url)
}

pub fn combined(
    data_files: &[DataFile],
    audio: &AudioManifest,
    description: Option<&str>,
    url: Option<&str>,
) -> String {
    generate(data_files, audio, description, url)
}

fn generate(
    data_files: &[DataFile],
    audio: &AudioManifest,
    description: Option<&str>,
    url: Option<&str>,
) -> String {
    let mut out = String::from("fx_version 'cerulean'\ngame 'gta5'\n");

    if let Some(desc) = description {
        out.push_str(&format!("\ndescription {}\n", quote_lua(desc)));
    }
    if let Some(u) = url {
        out.push_str(&format!("url {}\n", quote_lua(u)));
    }

    let has_data = !data_files.is_empty();
    let has_audio = !audio.is_empty();
    if has_data || has_audio {
        out.push_str("\nfiles {\n");
        let extensions: BTreeSet<&str> = data_files
            .iter()
            .filter_map(|file| Path::new(&file.path).extension()?.to_str())
            .collect();
        for ext in extensions {
            out.push_str(&format!("    'data/**/*.{}',\n", ext));
        }
        if has_audio {
            out.push_str("    'sfx/**/*.awc',\n");
            for phys in &audio.physical_files {
                out.push_str(&format!("    {},\n", quote_lua(phys)));
            }
        }
        out.push_str("}\n");
    }

    for file in data_files {
        out.push_str(&format!(
            "\ndata_file '{}' {}",
            file.directive,
            quote_lua(&file.path)
        ));
    }
    if !data_files.is_empty() {
        out.push('\n');
    }

    append_audio_directives(&mut out, audio);
    out
}

fn append_audio_directives(out: &mut String, audio: &AudioManifest) {
    for (game, sound) in &audio.game_sound_data {
        out.push_str(&format!("\ndata_file 'AUDIO_GAMEDATA' {}", quote_lua(game)));
        out.push_str(&format!(
            "\ndata_file 'AUDIO_SOUNDDATA' {}",
            quote_lua(sound)
        ));
    }
    for wp in &audio.wavepacks {
        out.push_str(&format!("\ndata_file 'AUDIO_WAVEPACK' {}", quote_lua(wp)));
    }
    if !audio.game_sound_data.is_empty() || !audio.wavepacks.is_empty() {
        out.push('\n');
    }
}

fn quote_lua(s: &str) -> String {
    if s.contains('\'') && !s.contains('"') {
        format!("\"{}\"", s)
    } else {
        format!("'{}'", s.replace('\'', "\\'"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infers_world_of_variety_directives() {
        assert_eq!(
            directive_for("data/vehicles/main.meta"),
            Some("VEHICLE_METADATA_FILE")
        );
        assert_eq!(
            directive_for("data/peds/main.meta"),
            Some("PED_METADATA_FILE")
        );
        assert_eq!(
            directive_for("data/clipsets/main.ymt"),
            Some("CLIP_SETS_FILE")
        );
        assert_eq!(directive_for("data/popcycle.dat"), Some("POPSCHED_FILE"));
    }

    #[test]
    fn rejects_unknown_or_unsupported_files() {
        assert_eq!(directive_for("data/scenario/region/davis.ymt"), None);
        assert_eq!(directive_for("data/dispatch.meta"), None);
        assert_eq!(directive_for("data/peds.ymt"), None);
        assert_eq!(directive_for("data/vehicles/arbitrary.xml"), None);
        assert_eq!(directive_for("data/unknown.meta"), None);
    }
}
