#![allow(unused_imports)]
use lazy_static::lazy_static;
use regex::Regex;
use std::collections::HashSet;
use std::ffi::OsStr;
use std::fs::{create_dir, File};
use std::io::prelude::Write;
use std::io::BufReader;
use std::io::Error as IOError;
use std::path::Path;
use unreal_asset::{
    base::types::PackageIndex,
    cast,
    engine_version::EngineVersion,
    exports::{Export, ExportBaseTrait, ExportNormalTrait, NormalExport},
    properties::{object_property::ObjectProperty, Property},
    Asset,
};

lazy_static! {
    static ref RE_INDEX: Regex = Regex::new(r"([^_]index: )(-?[1-9][0-9]*)").unwrap();
    static ref SUPPORTED_EXTENSIONS: HashSet<String> = vec!["uasset", "umap"]
        .iter()
        .map(|s| s.to_string())
        .collect();
}

const GLOBAL_STYLE: &str = "<style>a{text-decoration:none}a:visited{color:darkmagenta}</style>";

fn link_and_transform_indices(haystack: &str, transform: impl Fn(i32) -> String) -> String {
    let mut result = String::with_capacity(haystack.len());
    let mut last_match = 0;
    for caps in RE_INDEX.captures_iter(haystack) {
        let m = caps.get(0).unwrap();
        result.push_str(&haystack[last_match..m.start()]);
        result.push_str(caps.get(1).unwrap().as_str());
        let index = i32::from_str_radix(caps.get(2).unwrap().as_str(), 10).unwrap();
        result += &transform(index);
        last_match = m.end();
    }
    result.push_str(&haystack[last_match..]);
    result
}

#[test]
fn test_link_and_transform_indices() {
    assert_eq!(
        " index: 42  _index: 1  index: -42  etc".to_string(),
        link_and_transform_indices(" index: 21  _index: 1  index: -21  etc", |i| (i * 2)
            .to_string())
    );
}

fn try_create_dir<P: AsRef<Path>>(path: P) -> std::io::Result<()> {
    let path = path.as_ref();
    if path.exists() && path.is_dir() {
        Ok(())
    } else {
        create_dir(path)
    }
}

fn print_usage() {
    eprintln!("Please pass in at least one uasset. Example:");
    eprintln!("> ./uasset-index path/to/my_uasset.uasset");
}

fn is_valid_extension(ext: Option<&OsStr>) -> bool {
    match ext {
        Some(ext) => ext == "uasset" || ext == "umap",
        None => false,
    }
}

fn main() {
    let mut args = std::env::args();
    _ = args.next();
    let paths: Vec<String> = args.collect();
    if paths.len() == 0 {
        print_usage();
        return;
    }
    for path in paths {
        let path = Path::new(&path);
        index(path);
    }
}

fn index(path: &Path) {
    if path.is_dir() {
        let _ = index_dir(path);
    } else if path.is_file() {
        index_file(path);
    }
}

fn index_dir(path: &Path) -> Result<(), IOError> {
    println!("Indexing directory: {}", path.to_str().unwrap());
    let mut known_index_dirs = HashSet::new();
    for entry in std::fs::read_dir(path).unwrap() {
        let entry = entry?;
        let path = entry.path();
        if !entry.file_type()?.is_file() {
            continue;
        }
        if !SUPPORTED_EXTENSIONS.contains(path.extension().unwrap().to_str().unwrap()) {
            continue;
        }
        index_file(&path);
        known_index_dirs.insert(path.with_extension("").to_string_lossy().to_string());
    }
    for entry in std::fs::read_dir(path).unwrap() {
        let entry = entry?;
        let path = entry.path();
        if !entry.file_type()?.is_dir() {
            continue;
        }
        if known_index_dirs.contains(path.to_str().unwrap()) {
            continue;
        }
        index_dir(&path)?;
    }
    Ok(())
}

fn index_file(path: &Path) {
    println!(
        "Indexing uasset file: {}",
        path.file_name().unwrap().to_str().unwrap()
    );
    if !is_valid_extension(path.extension()) {
        eprintln!("Invalid extension. Valid extensions are: 'umap', 'uasset'");
        return;
    }
    if !path.exists() {
        eprintln!("File does not exist: {}", path.display());
        return;
    }
    let uexp_path = path.with_extension("uexp");

    let uasset_file = File::open(path).unwrap();
    let maybe_uexp_file = File::open(uexp_path).ok();

    let asset = Asset::new(uasset_file, maybe_uexp_file, EngineVersion::VER_UE5_1, None).unwrap();

    let uasset_name = path.file_stem().unwrap();
    let main_dir = path.parent().unwrap().join(uasset_name);
    let exports_dir = main_dir.join("exports");
    let imports_dir = main_dir.join("imports");
    try_create_dir(&main_dir).expect("Failed to create main directory.");
    try_create_dir(&exports_dir).expect("Failed to create exports directory.");
    try_create_dir(&imports_dir).expect("Failed to create imports directory.");

    let mut main_index =
        File::create(main_dir.join("index.html")).expect("Failed to create main index file.");
    main_index
        .write_all(GLOBAL_STYLE.as_bytes())
        .expect("Failed to write to main index file.");
    main_index
        .write_all(
            format!(
                "<h1>
        <a href=\"..\">.</a>/
        {}/
        </h1>
        <ul>
        <li><a href=\"imports\">imports</a></li>
        <li><a href=\"exports\">exports</a></li>
        </ul>",
                uasset_name.to_string_lossy()
            )
            .as_bytes(),
        )
        .expect("Failed to write to main index file.");

    let link_and_annotate_index = |index: i32| {
        if index == 0 {
            panic!("Tried to annotate 0 index.");
        }
        if index < 0 {
            let name = asset.imports[(-index - 1) as usize]
                .object_name
                .get_owned_content();
            format!(
                "<a href=\"../../imports/{pos_index}\">{neg_index} ({name})</a>",
                name = name,
                pos_index = -index,
                neg_index = index
            )
        } else {
            let name = asset.asset_data.exports[(index - 1) as usize]
                .get_base_export()
                .object_name
                .get_owned_content();
            format!(
                "<a href=\"../../exports/{index}\">{index} ({name})</a>",
                name = name,
                index = index
            )
        }
    };

    let mut exports_index =
        File::create(exports_dir.join("index.html")).expect("Failed to create exports index file.");
    let mut exports_index_contents = asset
        .asset_data
        .exports
        .iter()
        .enumerate()
        .map(|(i, export)| {
            format!(
                "<li><a href=\"{i}\">{i} ({name})</a></li>",
                i = i + 1,
                name = export.get_base_export().object_name.get_owned_content()
            )
        })
        .fold("<ul>".to_string(), |a, b| a + &b);
    exports_index_contents += "</ul>";
    exports_index
        .write_all(GLOBAL_STYLE.as_bytes())
        .expect("Failed to write to exports index file.");
    exports_index
        .write_all(
            format!(
                "<h1>
                <a href=\"../..\">.</a>/
                <a href=\"..\">{}</a>/
                exports
                </h1>",
                uasset_name.to_string_lossy()
            )
            .as_bytes(),
        )
        .expect("Failed to write to exports index file.");
    exports_index
        .write_all(exports_index_contents.as_bytes())
        .expect("Failed to write to exports index file.");
    for (i, export) in asset.asset_data.exports.iter().enumerate() {
        let dir = exports_dir.join((i + 1).to_string());
        try_create_dir(&dir).expect("Failed to create export directory.");
        let mut file =
            File::create(dir.join("index.html")).expect("Failed to create export HTML file.");
        let dump = format!(
            "<span style=\"white-space-collapse:preserve;font-family:monospace\">{:#?}</span>",
            export
        );
        let dump = link_and_transform_indices(&dump, link_and_annotate_index);
        file.write_all(GLOBAL_STYLE.as_bytes())
            .expect("Failed to write to export HTML file.");
        file.write_all(
            format!(
                "<h1>
                    <a href=\"../../..\">.</a>/
                    <a href=\"../..\">{base}</a>/
                    <a href=\"..\">exports</a>/
                    {i}
                    </h1>",
                base = uasset_name.to_string_lossy(),
                i = i + 1
            )
            .as_bytes(),
        )
        .expect("Failed to write to export HTML file.");
        file.write_all(dump.as_bytes())
            .expect("Failed to write to export HTML file.");
    }
    let mut imports_index =
        File::create(imports_dir.join("index.html")).expect("Failed to create imports index file.");
    let mut imports_index_contents = asset
        .imports
        .iter()
        .enumerate()
        .map(|(i, import)| {
            format!(
                "<li><a href=\"{i}\">{i} ({name})</a></li>",
                i = i + 1,
                name = import.object_name.get_owned_content()
            )
        })
        .fold("<ul>".to_string(), |a, b| a + &b);
    imports_index_contents += "</ul>";
    imports_index
        .write_all(GLOBAL_STYLE.as_bytes())
        .expect("Failed to write to imports index file.");
    imports_index
        .write_all(
            format!(
                "<h1>
                <a href=\"../..\">.</a>/
                <a href=\"..\">{}</a>/
                imports
                </h1>",
                uasset_name.to_string_lossy()
            )
            .as_bytes(),
        )
        .expect("Failed to write to imports index file.");
    imports_index
        .write_all(imports_index_contents.as_bytes())
        .expect("Failed to write to imports index file.");
    for (i, import) in asset.imports.iter().enumerate() {
        let dir = imports_dir.join((i + 1).to_string());
        try_create_dir(&dir).expect("Failed to create import directory.");
        let mut file =
            File::create(dir.join("index.html")).expect("Failed to create import HTML file.");
        let dump = format!(
            "<span style=\"white-space-collapse:preserve;font-family:monospace\">{:#?}</span>",
            import
        );
        let dump = link_and_transform_indices(&dump, link_and_annotate_index);
        file.write_all(GLOBAL_STYLE.as_bytes())
            .expect("Failed to write to import HTML file.");
        file.write_all(
            format!(
                "<h1>
                    <a href=\"../../..\">.</a>/
                    <a href=\"../..\">{base}</a>/
                    <a href=\"..\">imports</a>/
                    {i}
                    </h1>",
                base = uasset_name.to_string_lossy(),
                i = i + 1
            )
            .as_bytes(),
        )
        .expect("Failed to write to import HTML file.");
        file.write_all(dump.as_bytes())
            .expect("Failed to write to import HTML file.");
    }
}
