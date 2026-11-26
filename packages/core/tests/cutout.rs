use astral_core::render::cutout::{AstralCutoutFrame, Kind, corner_radius, kind, logo_frame};

const TOLERANCE: f64 = 0.001;

fn frame(x: f64, y: f64, width: f64, height: f64) -> AstralCutoutFrame {
    AstralCutoutFrame { x, y, width, height }
}

#[test]
fn classifies_cutouts_and_fits_the_logo() {
    assert!(kind(frame(0.0, 0.0, 0.0, 40.0)).is_none());
    let notch = frame(0.0, 0.0, 200.0, 30.0);
    assert!(kind(notch) == Some(Kind::Notch));
    assert!((corner_radius(Kind::Notch, notch) - 19.5).abs() < TOLERANCE);
    let island = frame(60.0, 10.0, 180.0, 48.0);
    assert!(kind(island) == Some(Kind::Island));
    assert!((corner_radius(Kind::Island, island) - 24.0).abs() < TOLERANCE);
    let hole = frame(80.0, 20.0, 40.0, 40.0);
    assert!(kind(hole) == Some(Kind::Hole));
    let logo = logo_frame(Kind::Hole, hole);
    assert!((logo.height - 24.0).abs() < TOLERANCE);
    assert!((logo.x - 5.0).abs() < 0.01);
    assert!((logo.y - 8.0).abs() < TOLERANCE);
}

#[test]
fn reads_slider_payloads_strictly() {
    let key = astral_core::wire::properties::current().value;
    let [low, high] = key.to_le_bytes();
    let mut payload = vec![8, 1, 0, 0, 0, low, high, 3, 7, 0, 0, 0];
    assert_eq!(astral_core::wire::events::event_value(&payload), Some(7.0));
    for size in 0..payload.len() {
        assert!(astral_core::wire::events::event_value(&payload[..size]).is_none());
    }
    payload.push(0);
    assert!(astral_core::wire::events::event_value(&payload).is_none());
    payload.pop();
    payload[7] = 5;
    payload.resize(16, 0);
    payload[8..16].copy_from_slice(&f64::NAN.to_le_bytes());
    assert!(astral_core::wire::events::event_value(&payload).is_none());
}
