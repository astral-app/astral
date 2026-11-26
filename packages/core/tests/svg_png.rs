use astral_core::render::svg::png;

#[test]
fn renders_an_icon_to_a_square_png() {
    let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M2 2h20v20H2z" fill="#fff"/></svg>"##;
    let data = png(svg, 48, 48).expect("png");
    assert_eq!(&data[..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(u32::from_be_bytes(data[16..20].try_into().unwrap()), 48);
    assert!(png(b"not svg", 48, 48).is_none());
    assert!(png(svg, 0, 48).is_none());
}
