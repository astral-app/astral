use astral_core::render::svg::{ARC, CLOSE, CUBIC, LINE, MOVE, QUAD, parse};

#[test]
fn parses_relative_commands_and_shorthands() {
    let segments = parse(b"M10 10 l5,0 h5 v5 c1 1 2 2 3 3 s4 4 5 5 q1 1 2 2 t3 3 z M0 0 1e1 -.5");
    assert_eq!(&segments[..9], &[MOVE, 10.0, 10.0, LINE, 15.0, 10.0, LINE, 20.0, 10.0]);
    assert_eq!(&segments[9..12], &[LINE, 20.0, 15.0]);
    assert_eq!(&segments[12..19], &[CUBIC, 21.0, 16.0, 22.0, 17.0, 23.0, 18.0]);
    assert_eq!(&segments[19..26], &[CUBIC, 24.0, 19.0, 27.0, 22.0, 28.0, 23.0]);
    assert_eq!(&segments[26..31], &[QUAD, 29.0, 24.0, 30.0, 25.0]);
    assert_eq!(&segments[31..36], &[QUAD, 31.0, 26.0, 33.0, 28.0]);
    assert_eq!(segments[36], CLOSE);
    assert_eq!(&segments[37..], &[MOVE, 0.0, 0.0, LINE, 10.0, -0.5]);
}

#[test]
fn arcs_and_bad_input() {
    let segments = parse(b"M0 0 A5 5 0 0 1 10 0 A0 3 0 1 1 20 0");
    assert_eq!(segments[3], ARC);
    assert!((segments[4] - 5.0).abs() < 1e-9 && segments[5].abs() < 1e-9);
    assert_eq!(&segments[11..], &[LINE, 20.0, 0.0]);
    assert!(parse(b"12 34").is_empty());
    assert_eq!(parse(b"M1 2 L"), vec![MOVE, 1.0, 2.0]);
}
