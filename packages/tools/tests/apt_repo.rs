use std::io::Write;

use astral_tools::apt_repo::{control, package_entry, release};

fn tar_with(name: &str, contents: &[u8]) -> Vec<u8> {
    let mut header = vec![0u8; 512];
    header[..name.len()].copy_from_slice(name.as_bytes());
    let size = format!("{:011o}\0", contents.len());
    header[124..136].copy_from_slice(size.as_bytes());
    let mut tar = header;
    tar.extend_from_slice(contents);
    tar.resize(tar.len().div_ceil(512) * 512 + 1024, 0);
    tar
}

fn ar_member(name: &str, body: &[u8]) -> Vec<u8> {
    let mut header = format!(
        "{name:<16}{:<12}{:<6}{:<6}{:<8}{:<10}`\n",
        0,
        0,
        0,
        "100644",
        body.len()
    )
    .into_bytes();
    header.extend_from_slice(body);
    if body.len() % 2 == 1 {
        header.push(b'\n');
    }
    header
}

fn deb(control_text: &str) -> Vec<u8> {
    let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gzip.write_all(&tar_with("./control", control_text.as_bytes()))
        .unwrap();
    let mut archive = b"!<arch>\n".to_vec();
    archive.extend(ar_member("debian-binary", b"2.0\n"));
    archive.extend(ar_member("control.tar.gz", &gzip.finish().unwrap()));
    archive
}

#[test]
fn reads_the_control_file_and_indexes_the_package() {
    let package = deb("Package: app.astral.ios\nVersion: 0.0.1~alpha.3\n");
    let text = control(&package).unwrap();
    assert_eq!(text, "Package: app.astral.ios\nVersion: 0.0.1~alpha.3");
    let entry = package_entry(&text, "debs/a.deb", &package);
    assert!(entry.contains("Filename: debs/a.deb\n"));
    assert!(entry.contains(&format!("Size: {}\n", package.len())));
    assert!(entry.contains("SHA256: "));
}

#[test]
fn rejects_files_that_are_not_packages() {
    assert!(control(b"not a deb").is_err());
}

#[test]
fn release_lists_each_index_under_both_hashes() {
    let text = release(&[("Packages", b"abc")]);
    assert!(text.starts_with("Origin: astral\n"));
    assert_eq!(text.matches(" 3 Packages\n").count(), 2);
}
