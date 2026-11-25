use astral_core::host_requests::{Request, host_of, parse, path_of, rebuild};

fn string(out: &mut Vec<u8>, value: &str) {
    out.push(6);
    out.extend_from_slice(&(value.len() as u32).to_le_bytes());
    out.extend_from_slice(value.as_bytes());
}

fn frame(method: &str, url: &str, headers: &[(&str, &str)], body: &[u8]) -> Vec<u8> {
    let mut arguments = vec![8];
    arguments.extend_from_slice(&4u32.to_le_bytes());
    arguments.extend_from_slice(&0u16.to_le_bytes());
    string(&mut arguments, method);
    arguments.extend_from_slice(&1u16.to_le_bytes());
    string(&mut arguments, url);
    arguments.extend_from_slice(&2u16.to_le_bytes());
    arguments.push(3);
    arguments.extend_from_slice(&7u32.to_le_bytes());
    arguments.extend_from_slice(&3u16.to_le_bytes());
    arguments.push(7);
    arguments.extend_from_slice(&(headers.len() as u32).to_le_bytes());
    for (name, value) in headers {
        arguments.push(7);
        arguments.extend_from_slice(&2u32.to_le_bytes());
        string(&mut arguments, name);
        string(&mut arguments, value);
    }
    let mut out = vec![1, 0, 0, 0, 9, 9, 9, 9, 8, 8, 8, 8];
    out.extend_from_slice(&(arguments.len() as u32).to_le_bytes());
    out.extend_from_slice(&arguments);
    out.extend_from_slice(body);
    out
}

#[test]
fn parses_a_host_request() {
    let bytes = frame(
        "POST",
        "https://discord.com/api/v9/channels/1/messages?x=1",
        &[("Content-Type", "application/json")],
        b"{}",
    );
    let request = parse(&bytes).expect("request");
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/api/v9/channels/1/messages");
    assert_eq!(request.headers, vec![("content-type".to_owned(), "application/json".to_owned())]);
    assert_eq!(request.body, b"{}");
}

#[test]
fn rebuilds_with_changes_and_keeps_other_arguments() {
    let bytes = frame("POST", "https://discord.com/api/v9/x", &[("a", "1")], b"old");
    let changed = Request {
        method: "PATCH".into(),
        url: "https://discord.com/api/v9/y".into(),
        path: "/api/v9/y".into(),
        headers: vec![("b".into(), "2".into()), ("c".into(), "3".into())],
        body: b"new body".to_vec(),
    };
    let rebuilt = rebuild(&bytes, &changed);
    assert_eq!(&rebuilt[..12], &bytes[..12]);
    assert_eq!(parse(&rebuilt).expect("rebuilt"), changed);
    assert_eq!(rebuild(&bytes, &parse(&bytes).unwrap()), bytes);
}

#[test]
fn rejects_other_frames_and_reads_urls() {
    assert!(parse(&[0; 20]).is_none());
    assert!(parse(&frame("FETCH", "https://x", &[], b"")).is_none());
    assert_eq!(host_of("https://Discord.com:443/a"), "discord.com");
    assert_eq!(path_of("https://discord.com"), "/");
    assert_eq!(path_of("https://discord.com/a/b#c"), "/a/b");
}
