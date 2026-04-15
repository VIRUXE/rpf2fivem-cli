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

/// Generate fxmanifest.lua for a single resource, only including
/// data_file directives for meta files that are actually present.
pub fn single(meta_files: &[&str], description: Option<&str>) -> String {
    let mut out = String::from("fx_version 'cerulean'\ngame 'gta5'\n");

    if let Some(desc) = description {
        out.push_str(&format!("\ndescription '{}'\n", desc.replace('\'', "\\'")));
    }

    if !meta_files.is_empty() {
        out.push_str("\nfiles {\n    'data/*.meta'\n}\n");
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

    out
}

/// Generate fxmanifest.lua for a combined resource, only including
/// data_file directives for meta files that are actually present.
pub fn combined(meta_files: &[&str], description: Option<&str>) -> String {
    let mut out = String::from("fx_version 'cerulean'\ngame 'gta5'\n");

    if let Some(desc) = description {
        out.push_str(&format!("\ndescription '{}'\n", desc.replace('\'', "\\'")));
    }

    if !meta_files.is_empty() {
        out.push_str("\nfiles {\n    'data/**/*.meta'\n}\n");
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

    out
}
