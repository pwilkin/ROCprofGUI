use profile_core::code_object_files::{extract, parse, MAX_OBJECT_BYTES};
use sha2::{Digest, Sha256};
use std::sync::atomic::AtomicBool;

fn object() -> Vec<u8> {
    let mut bytes = vec![0; 173];
    bytes[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
    bytes[18..20].copy_from_slice(&224u16.to_le_bytes());
    bytes[64..].fill(37);
    bytes
}

#[test]
fn uri_ranges_preserve_u64_and_decode_paths_without_form_semantics() {
    let range = parse("file:///a%20b/%C5%BC%2B.co?offset=0X20000000000001&size=0100").unwrap();
    assert_eq!(range.path.to_str().unwrap(), "/a b/ż+.co");
    assert_eq!(range.offset, 9007199254740993);
    assert_eq!(range.size, Some(64));
    assert_eq!(parse("file:///a#offset=0").unwrap().size, None);
    assert_eq!(parse("file:///a").unwrap().offset, 0);
    assert_eq!(parse("file:///a#size=64").unwrap().size, Some(64));
    for uri in [
        "memory://1#offset=0&size=64",
        "https://a",
        "file://host/a",
        "file:////host/a",
        "file:///a%",
        "file:///a%00",
        "file:///a%FF",
        "file:///a+b",
        "file:///a#",
        "file:///a#offset=1&offset=2",
        "file:///a#offset=-1",
        "file:///a#offset=+1",
        "file:///a#offset=08",
        "file:///a#offset=0x",
        "file:///a#size=1&foo=2",
        "file:///a#offset=18446744073709551616",
    ] {
        assert!(parse(uri).is_err(), "{uri}");
    }
}

#[test]
fn extracts_only_recorded_bytes_and_does_not_replace_existing_paths() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("host");
    let destination = dir.path().join("kernel.co");
    let bytes = object();
    let mut host = vec![9; 32];
    host.extend_from_slice(&bytes);
    host.extend_from_slice(&[8; 64]);
    std::fs::write(&source, &host).unwrap();
    let uri = format!(
        "file://{}#offset=0x20&size={}",
        source.display(),
        bytes.len()
    );
    let flag = AtomicBool::new(false);
    let result = extract(&uri, &destination, &flag).unwrap();
    assert_eq!(std::fs::read(&destination).unwrap(), bytes);
    assert_eq!(result.bytes, "173");
    assert_eq!(result.offset, "32");
    assert_eq!(result.sha256, format!("{:x}", Sha256::digest(&bytes)));
    assert!(extract(&uri, &destination, &flag).is_err());
    assert!(extract(&uri, &source, &flag).is_err());
    assert_eq!(std::fs::read(&source).unwrap(), host);
    assert_eq!(std::fs::read(&destination).unwrap(), bytes);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
}

#[test]
fn rejects_invalid_ranges_content_and_cancelled_exports_without_outputs() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("kernel");
    let output = dir.path().join("saved.co");
    std::fs::write(&source, object()).unwrap();
    let uri = format!("file://{}", source.display());
    let flag = AtomicBool::new(false);
    for suffix in [
        "#size=0".to_string(),
        "#size=63".into(),
        "#offset=1".into(),
        "#offset=18446744073709551615&size=64".into(),
        format!("#size={}", MAX_OBJECT_BYTES + 1),
        "#offset=1&size=64".into(),
    ] {
        assert!(extract(&format!("{uri}{suffix}"), &output, &flag).is_err());
        assert!(!output.exists());
    }
    assert!(extract(&uri, &output, &AtomicBool::new(true)).is_err());
    assert!(!output.exists());
    extract(&uri, &output, &flag).unwrap();
    assert_eq!(std::fs::read(&output).unwrap(), object());
}

#[test]
fn relocated_container_keeps_recorded_range_and_provenance() {
    use profile_core::code_object_files::extract_from;
    let dir = tempfile::tempdir().unwrap();
    let container = dir.path().join("relocated host");
    let output = dir.path().join("object.co");
    let bytes = object();
    let mut host = vec![0; 128];
    host.extend_from_slice(&bytes);
    host.extend_from_slice(&[42; 16]);
    std::fs::write(&container, &host).unwrap();
    let uri = "file:///unavailable-machine/host#offset=0200&size=173";
    let flag = AtomicBool::new(false);
    assert!(extract(uri, &output, &flag).is_err());
    let result = extract_from(uri, Some(&container), &output, &flag).unwrap();
    assert!(result.remapped);
    assert_eq!(result.recorded_uri, uri);
    assert_eq!(result.source, container.display().to_string());
    assert_eq!(result.offset, "128");
    assert_eq!(std::fs::read(&output).unwrap(), bytes);
    assert_eq!(std::fs::read(&container).unwrap(), host);
    let invalid_output = dir.path().join("invalid.co");
    assert!(extract_from(uri, Some(&output), &invalid_output, &flag).is_err());
    assert!(!invalid_output.exists());
    assert!(extract_from(
        "memory://1#offset=128&size=173",
        Some(&container),
        &invalid_output,
        &flag
    )
    .is_err());
    assert!(extract_from(
        uri,
        Some(std::path::Path::new("relative")),
        &invalid_output,
        &flag
    )
    .is_err());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
}
