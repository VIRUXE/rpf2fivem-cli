/// Map from meta filename to its FiveM data_file type.
const META_DIRECTIVES: &[(&str, &str)] = &[
    ("handling.meta",         "HANDLING_FILE"),
    ("vehicles.meta",         "VEHICLE_METADATA_FILE"),
    ("vehiclelayouts.meta",   "VEHICLE_LAYOUTS_FILE"),
    ("carcols.meta",          "CARCOLS_FILE"),
    ("carvariations.meta",    "VEHICLE_VARIATION_FILE"),
    ("dlctext.meta",          "DLCTEXT_FILE"),
    ("contentunlocks.meta",   "CARCONTENTUNLOCKS_FILE"),
    ("vehiclemodelsets.meta", "VEHICLE_METADATA_FILE"),
];

/// Audio layout discovered after extraction (paths use `/` and are relative to the resource root).
pub struct AudioManifest {
    /// `AUDIO_WAVEPACK` folder paths, e.g. `sfx/dlc_mycar`
    pub wavepacks: Vec<String>,
    /// Physical .rel/.dat files that must be in the 'files' section.
    pub physical_files: Vec<String>,
    /// Matched `(AUDIO_GAMEDATA path, AUDIO_SOUNDDATA path)` pairs for the data_file section.
    /// These are the "clean" versions (e.g. .dat instead of .dat151.rel).
    pub game_sound_data: Vec<(String, String)>,
}

impl AudioManifest {
    pub fn is_empty(&self) -> bool {
        self.wavepacks.is_empty() && self.game_sound_data.is_empty()
    }
}

/// Generate fxmanifest.lua for a single resource, only including
/// data_file directives for meta files that are actually present.
pub fn single(
    meta_files: &[&str],
    audio: &AudioManifest,
    description: Option<&str>,
    url: Option<&str>,
) -> String {
    let mut out = String::from("fx_version 'cerulean'\ngame 'gta5'\n");

    if let Some(desc) = description {
        out.push_str(&format!("\ndescription '{}'\n", desc.replace('\'', "\\'")));
    }

    if let Some(u) = url {
        out.push_str(&format!("url '{}'\n", u.replace('\'', "\\'")));
    }

    let has_meta = !meta_files.is_empty();
    let has_audio = !audio.is_empty();
    if has_meta || has_audio {
        out.push_str("\nfiles {\n");
        if has_meta {
            out.push_str("    'data/*.meta',\n");
        }
        if has_audio {
            out.push_str("    'sfx/**/*.awc',\n");
            for phys in &audio.physical_files {
                out.push_str(&format!("    '{}',\n", phys.replace('\'', "\\'")));
            }
        }
        out.push_str("}\n");
    }

    if has_meta {
        for &name in meta_files {
            if let Some(&(_, directive)) = META_DIRECTIVES.iter().find(|&&(n, _)| n == name) {
                out.push_str(&format!(
                    "\ndata_file '{}' 'data/{}'",
                    directive, name
                ));
            }
        }
        out.push('\n');
    }

    append_audio_directives(&mut out, audio);

    out
}

/// Generate fxmanifest.lua for a combined resource, only including
/// data_file directives for meta files that are actually present.
pub fn combined(
    meta_files: &[&str],
    audio: &AudioManifest,
    description: Option<&str>,
    url: Option<&str>,
) -> String {
    let mut out = String::from("fx_version 'cerulean'\ngame 'gta5'\n");

    if let Some(desc) = description {
        out.push_str(&format!("\ndescription '{}'\n", desc.replace('\'', "\\'")));
    }

    if let Some(u) = url {
        out.push_str(&format!("url '{}'\n", u.replace('\'', "\\'")));
    }

    let has_meta = !meta_files.is_empty();
    let has_audio = !audio.is_empty();
    if has_meta || has_audio {
        out.push_str("\nfiles {\n");
        if has_meta {
            out.push_str("    'data/**/*.meta',\n");
        }
        if has_audio {
            out.push_str("    'sfx/**/*.awc',\n");
            for phys in &audio.physical_files {
                out.push_str(&format!("    '{}',\n", phys.replace('\'', "\\'")));
            }
        }
        out.push_str("}\n");
    }

    if has_meta {
        for &name in meta_files {
            if let Some(&(_, directive)) = META_DIRECTIVES.iter().find(|&&(n, _)| n == name) {
                out.push_str(&format!(
                    "\ndata_file '{}' 'data/**/{}'",
                    directive, name
                ));
            }
        }
        out.push('\n');
    }

    append_audio_directives(&mut out, audio);

    out
}

fn append_audio_directives(out: &mut String, audio: &AudioManifest) {
    for (game, sound) in &audio.game_sound_data {
        out.push_str(&format!(
            "\ndata_file 'AUDIO_GAMEDATA' '{}'",
            game.replace('\'', "\\'")
        ));
        out.push_str(&format!(
            "\ndata_file 'AUDIO_SOUNDDATA' '{}'",
            sound.replace('\'', "\\'")
        ));
    }
    for wp in &audio.wavepacks {
        out.push_str(&format!(
            "\ndata_file 'AUDIO_WAVEPACK' '{}'",
            wp.replace('\'', "\\'")
        ));
    }
    if !audio.game_sound_data.is_empty() || !audio.wavepacks.is_empty() {
        out.push('\n');
    }
}
