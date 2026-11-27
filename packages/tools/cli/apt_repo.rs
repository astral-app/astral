use std::io::{Read, Write};
use std::path::Path;

use sha2::{Digest, Sha256, Sha512};

const AR_MAGIC: &[u8] = b"!<arch>\n";
const AR_HEADER: usize = 60;
const AR_NAME: usize = 16;
const AR_SIZE_START: usize = 48;
const AR_SIZE_END: usize = 58;
const CONTROL_ARCHIVE: &str = "control.tar.gz";
const TAR_BLOCK: usize = 512;
const TAR_SIZE_START: usize = 124;
const TAR_SIZE_END: usize = 136;
const CONTROL_FILE: &str = "control";
const DEBS: &str = "debs";
const RELEASE_FIELDS: [(&str, &str); 7] = [
    ("Origin", "astral"),
    ("Label", "astral"),
    ("Suite", "stable"),
    ("Codename", "ios"),
    ("Architectures", "iphoneos-arm64"),
    ("Components", "main"),
    ("Description", "astral alpha builds"),
];

fn ar_member<'a>(archive: &'a [u8], wanted: &str) -> Option<&'a [u8]> {
    let mut offset = archive.strip_prefix(AR_MAGIC).map(|_| AR_MAGIC.len())?;
    while offset + AR_HEADER <= archive.len() {
        let header = &archive[offset..offset + AR_HEADER];
        let name = String::from_utf8_lossy(&header[..AR_NAME]);
        let size: usize = String::from_utf8_lossy(&header[AR_SIZE_START..AR_SIZE_END])
            .trim()
            .parse()
            .ok()?;
        let start = offset + AR_HEADER;
        let body = archive.get(start..start + size)?;
        if name.trim().trim_end_matches('/') == wanted {
            return Some(body);
        }
        offset = start + size + size % 2;
    }
    None
}

fn tar_file(archive: &[u8], wanted: &str) -> Option<Vec<u8>> {
    let mut offset = 0;
    while offset + TAR_BLOCK <= archive.len() {
        let header = &archive[offset..offset + TAR_BLOCK];
        if header.iter().all(|byte| *byte == 0) {
            return None;
        }
        let name_end = header[..100]
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(100);
        let name = String::from_utf8_lossy(&header[..name_end]);
        let size_text = String::from_utf8_lossy(&header[TAR_SIZE_START..TAR_SIZE_END]);
        let size = usize::from_str_radix(
            size_text.trim_matches(|character: char| character == '\0' || character == ' '),
            8,
        )
        .ok()?;
        let start = offset + TAR_BLOCK;
        if name.trim_start_matches("./") == wanted {
            return archive.get(start..start + size).map(<[u8]>::to_vec);
        }
        offset = start + size.div_ceil(TAR_BLOCK) * TAR_BLOCK;
    }
    None
}

pub fn control(deb: &[u8]) -> Result<String, String> {
    let compressed = ar_member(deb, CONTROL_ARCHIVE).ok_or("the package has no control.tar.gz")?;
    let mut tar = Vec::new();
    flate2::read::GzDecoder::new(compressed)
        .read_to_end(&mut tar)
        .map_err(|error| error.to_string())?;
    let control = tar_file(&tar, CONTROL_FILE).ok_or("control.tar.gz has no control file")?;
    Ok(String::from_utf8_lossy(&control).trim_end().to_owned())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn package_entry(control: &str, filename: &str, deb: &[u8]) -> String {
    format!(
        "{control}\nFilename: {filename}\nSize: {}\nSHA256: {}\nSHA512: {}\n",
        deb.len(),
        hex(&Sha256::digest(deb)),
        hex(&Sha512::digest(deb))
    )
}

pub fn release(files: &[(&str, &[u8])]) -> String {
    let mut out: String = RELEASE_FIELDS
        .iter()
        .map(|(key, value)| format!("{key}: {value}\n"))
        .collect();
    for (title, digest) in [("SHA256", 0), ("SHA512", 1)] {
        out.push_str(&format!("{title}:\n"));
        for (name, contents) in files {
            let sum = if digest == 0 {
                hex(&Sha256::digest(contents))
            } else {
                hex(&Sha512::digest(contents))
            };
            out.push_str(&format!(" {sum} {} {name}\n", contents.len()));
        }
    }
    out
}

fn write(path: &Path, contents: &[u8]) -> Result<(), String> {
    std::fs::write(path, contents).map_err(|error| format!("{}: {error}", path.display()))
}

pub fn run(arguments: &[String]) -> Result<(), String> {
    let [directory] = arguments else {
        return Err("usage: astral apt-repo <repo directory with debs/>".into());
    };
    let root = Path::new(directory);
    let mut debs: Vec<_> = std::fs::read_dir(root.join(DEBS))
        .map_err(|error| format!("{}: {error}", root.join(DEBS).display()))?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "deb"))
        .collect();
    debs.sort();
    let mut packages = String::new();
    for path in &debs {
        let deb = std::fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        let control = control(&deb).map_err(|error| format!("{}: {error}", path.display()))?;
        packages.push_str(&package_entry(&control, &format!("{DEBS}/{name}"), &deb));
        packages.push('\n');
    }
    let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    gzip.write_all(packages.as_bytes())
        .map_err(|error| error.to_string())?;
    let compressed = gzip.finish().map_err(|error| error.to_string())?;
    write(&root.join("Packages"), packages.as_bytes())?;
    write(&root.join("Packages.gz"), &compressed)?;
    write(
        &root.join("Release"),
        release(&[
            ("Packages", packages.as_bytes()),
            ("Packages.gz", &compressed),
        ])
        .as_bytes(),
    )?;
    println!("indexed {} packages in {}", debs.len(), root.display());
    Ok(())
}
