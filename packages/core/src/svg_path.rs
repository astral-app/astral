use core::ffi::c_char;
use std::f64::consts::PI;

use crate::bridge::{AstralRustString, bytes};

pub const MOVE: f64 = 0.0;
pub const LINE: f64 = 1.0;
pub const CUBIC: f64 = 2.0;
pub const QUAD: f64 = 3.0;
pub const ARC: f64 = 4.0;
pub const CLOSE: f64 = 5.0;

struct Scanner<'a> {
    text: &'a [u8],
    offset: usize,
}

impl Scanner<'_> {
    fn skip_separators(&mut self) {
        while self
            .text
            .get(self.offset)
            .is_some_and(|byte| matches!(byte, b' ' | b',' | b'\n' | b'\t' | b'\r'))
        {
            self.offset += 1;
        }
    }

    fn digits(&mut self) -> usize {
        let start = self.offset;
        while self.text.get(self.offset).is_some_and(u8::is_ascii_digit) {
            self.offset += 1;
        }
        self.offset - start
    }

    fn number(&mut self) -> Option<f64> {
        self.skip_separators();
        let start = self.offset;
        if self.text.get(self.offset).is_some_and(|byte| matches!(byte, b'+' | b'-')) {
            self.offset += 1;
        }
        let mut count = self.digits();
        if self.text.get(self.offset) == Some(&b'.') {
            self.offset += 1;
            count += self.digits();
        }
        if count == 0 {
            self.offset = start;
            return None;
        }
        if self.text.get(self.offset).is_some_and(|byte| matches!(byte, b'e' | b'E')) {
            let mark = self.offset;
            self.offset += 1;
            if self.text.get(self.offset).is_some_and(|byte| matches!(byte, b'+' | b'-')) {
                self.offset += 1;
            }
            if self.digits() == 0 {
                self.offset = mark;
            }
        }
        std::str::from_utf8(&self.text[start..self.offset]).ok()?.parse().ok()
    }

    fn flag(&mut self) -> Option<bool> {
        self.skip_separators();
        let flag = match self.text.get(self.offset)? {
            b'0' => false,
            b'1' => true,
            _ => return None,
        };
        self.offset += 1;
        Some(flag)
    }

    fn numbers<const N: usize>(&mut self) -> Option<[f64; N]> {
        let mut values = [0.0; N];
        for value in &mut values {
            *value = self.number()?;
        }
        Some(values)
    }
}

fn arc(out: &mut Vec<f64>, from: (f64, f64), radius: (f64, f64), rotation: f64, large: bool, sweep: bool, to: (f64, f64)) {
    let (mut rx, mut ry) = radius;
    if rx == 0.0 || ry == 0.0 {
        out.extend([LINE, to.0, to.1]);
        return;
    }
    rx = rx.abs();
    ry = ry.abs();
    let phi = rotation * PI / 180.0;
    let (sin, cos) = phi.sin_cos();
    let (dx, dy) = ((from.0 - to.0) / 2.0, (from.1 - to.1) / 2.0);
    let (x1, y1) = (cos * dx + sin * dy, -sin * dx + cos * dy);
    let lambda = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry);
    if lambda > 1.0 {
        rx *= lambda.sqrt();
        ry *= lambda.sqrt();
    }
    let numerator = rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1;
    let denominator = rx * rx * y1 * y1 + ry * ry * x1 * x1;
    let factor = if denominator == 0.0 {
        0.0
    } else {
        (numerator / denominator).max(0.0).sqrt() * if large == sweep { -1.0 } else { 1.0 }
    };
    let (cx1, cy1) = (factor * rx * y1 / ry, -factor * ry * x1 / rx);
    let cx = cos * cx1 - sin * cy1 + (from.0 + to.0) / 2.0;
    let cy = sin * cx1 + cos * cy1 + (from.1 + to.1) / 2.0;
    let start = ((y1 - cy1) / ry).atan2((x1 - cx1) / rx);
    let end = ((-y1 - cy1) / ry).atan2((-x1 - cx1) / rx);
    let mut delta = end - start;
    if sweep && delta < 0.0 {
        delta += 2.0 * PI;
    }
    if !sweep && delta > 0.0 {
        delta -= 2.0 * PI;
    }
    out.extend([ARC, cx, cy, rx, ry, phi, start, delta]);
}

pub fn parse(data: &[u8]) -> Vec<f64> {
    let mut out = Vec::new();
    let mut scanner = Scanner { text: data, offset: 0 };
    let (mut current, mut start, mut control) = ((0.0, 0.0), (0.0, 0.0), (0.0, 0.0));
    let (mut command, mut previous) = (0u8, 0u8);
    loop {
        scanner.skip_separators();
        let Some(&character) = scanner.text.get(scanner.offset) else {
            break;
        };
        if character.is_ascii_alphabetic() {
            command = character;
            scanner.offset += 1;
        } else if command == 0 {
            break;
        }
        let relative = command.is_ascii_lowercase();
        let upper = command.to_ascii_uppercase();
        let origin = if relative { current } else { (0.0, 0.0) };
        let before = scanner.offset;
        match upper {
            b'Z' => {
                out.push(CLOSE);
                current = start;
                previous = upper;
                command = 0;
                continue;
            }
            b'M' => {
                let Some([a, b]) = scanner.numbers() else { break };
                current = (origin.0 + a, origin.1 + b);
                out.extend([MOVE, current.0, current.1]);
                start = current;
                command = if relative { b'l' } else { b'L' };
            }
            b'L' => {
                let Some([a, b]) = scanner.numbers() else { break };
                current = (origin.0 + a, origin.1 + b);
                out.extend([LINE, current.0, current.1]);
            }
            b'H' => {
                let Some(a) = scanner.number() else { break };
                current.0 = origin.0 + a;
                out.extend([LINE, current.0, current.1]);
            }
            b'V' => {
                let Some(a) = scanner.number() else { break };
                current.1 = origin.1 + a;
                out.extend([LINE, current.0, current.1]);
            }
            b'C' => {
                let Some([a, b, c, d, e, f]) = scanner.numbers() else { break };
                control = (origin.0 + c, origin.1 + d);
                current = (origin.0 + e, origin.1 + f);
                out.extend([CUBIC, origin.0 + a, origin.1 + b, control.0, control.1, current.0, current.1]);
                previous = upper;
                continue;
            }
            b'S' => {
                let Some([c, d, e, f]) = scanner.numbers() else { break };
                let first = if matches!(previous, b'C' | b'S') {
                    (2.0 * current.0 - control.0, 2.0 * current.1 - control.1)
                } else {
                    current
                };
                control = (origin.0 + c, origin.1 + d);
                current = (origin.0 + e, origin.1 + f);
                out.extend([CUBIC, first.0, first.1, control.0, control.1, current.0, current.1]);
                previous = upper;
                continue;
            }
            b'Q' => {
                let Some([a, b, e, f]) = scanner.numbers() else { break };
                control = (origin.0 + a, origin.1 + b);
                current = (origin.0 + e, origin.1 + f);
                out.extend([QUAD, control.0, control.1, current.0, current.1]);
                previous = upper;
                continue;
            }
            b'T' => {
                let Some([e, f]) = scanner.numbers() else { break };
                control = if matches!(previous, b'Q' | b'T') {
                    (2.0 * current.0 - control.0, 2.0 * current.1 - control.1)
                } else {
                    current
                };
                current = (origin.0 + e, origin.1 + f);
                out.extend([QUAD, control.0, control.1, current.0, current.1]);
                previous = upper;
                continue;
            }
            b'A' => {
                let (Some([a, b, c]), Some(large), Some(sweep), Some([f, g])) =
                    (scanner.numbers(), scanner.flag(), scanner.flag(), scanner.numbers())
                else {
                    break;
                };
                let target = (origin.0 + f, origin.1 + g);
                arc(&mut out, current, (a, b), c, large, sweep, target);
                current = target;
            }
            _ => break,
        }
        previous = upper;
        if scanner.offset == before {
            break;
        }
    }
    out
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn astral_svg_path(data: *const c_char, length: usize) -> AstralRustString {
    let segments = parse(bytes(data, length));
    AstralRustString::new(segments.iter().flat_map(|value| value.to_le_bytes()).collect())
}
