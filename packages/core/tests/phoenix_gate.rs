use astral_core::diagnostics::phoenix_gate::{RETURN_TRUE, evaluators, experiment_offsets};

const BASE: u64 = 0x1000;
const DESCRIPTOR: u64 = 0x5080;

fn write_words(text: &mut [u8], offset: usize, words: &[u32]) {
    for (index, word) in words.iter().enumerate() {
        text[offset + index * 4..offset + index * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
}

fn fixture(indexed: bool) -> (Vec<u8>, Vec<u64>) {
    let mut text = vec![0; 0x300];
    let guard = if indexed {
        [
            0x5292_1508,
            0xf868_6808,
            0xf100_091f,
            0x5400_00e0,
            0x5294_2308,
            0xf868_6808,
            0xb100_051f,
            0x5400_0060,
            0x395b_b408,
            0x3600_0068,
            0x5280_0040,
            0xd65f_03c0,
        ]
    } else {
        [
            0xf940_0008,
            0xf940_0108,
            0xf940_0109,
            0xf100_093f,
            0x5400_00c0,
            0xf946_0508,
            0xb100_051f,
            0x5400_0060,
            0x3960_f508,
            0x3600_0068,
            0x5280_0040,
            0xd65f_03c0,
        ]
    };
    for offset in [0, 0x100] {
        write_words(&mut text, offset, &guard);
    }
    write_words(&mut text, 0x140, &[0x9000_0033, 0x9102_0273]);
    for offset in [0x200, 0x220, 0x240] {
        write_words(&mut text, offset, &[0x9000_0033, 0x9102_0273]);
    }
    (text, vec![BASE, BASE + 0x100, BASE + 0x200])
}

#[test]
fn follows_the_experiment_descriptor_instead_of_the_most_frequent_helper() {
    for indexed in [false, true] {
        let (text, starts) = fixture(indexed);
        assert_eq!(evaluators(&text, BASE, &starts, &[DESCRIPTOR]).unwrap(), [BASE, BASE + 0x100]);
    }
}

#[test]
fn recognizes_register_offset_byte_loads_in_android_guards() {
    let (mut text, starts) = fixture(true);
    let guard = [
        0x5291_c508,
        0x8b08_0008,
        0xf940_0109,
        0xf100_093f,
        0x5400_00e0,
        0xf947_a908,
        0xb100_051f,
        0x5400_0080,
        0x5284_25a8,
        0x3868_6808,
        0x3600_0068,
        0x5280_0040,
        0xd65f_03c0,
    ];
    for offset in [0, 0x100] {
        write_words(&mut text, offset, &guard);
    }
    assert_eq!(evaluators(&text, BASE, &starts, &[DESCRIPTOR]).unwrap(), [BASE, BASE + 0x100]);
    for offset in [0, 0x100] {
        text[offset..offset + 8].copy_from_slice(&RETURN_TRUE);
    }
    assert_eq!(evaluators(&text, BASE, &starts, &[DESCRIPTOR]).unwrap(), [BASE, BASE + 0x100]);
}

#[test]
fn recognizes_both_evaluators_after_their_entries_have_been_patched() {
    let (mut text, starts) = fixture(true);
    for offset in [0, 0x100] {
        text[offset..offset + 8].copy_from_slice(&RETURN_TRUE);
    }
    assert_eq!(evaluators(&text, BASE, &starts, &[DESCRIPTOR]).unwrap(), [BASE, BASE + 0x100]);
}

#[test]
fn refuses_unanchored_incomplete_and_ambiguous_pairs() {
    let (mut text, starts) = fixture(true);
    assert!(evaluators(&text, BASE, &starts, &[DESCRIPTOR + 8]).is_err());
    let guard = text[..48].to_vec();
    text[0x200..0x230].copy_from_slice(&guard);
    assert!(evaluators(&text, BASE, &starts, &[DESCRIPTOR]).is_err());
    text[..48].fill(0);
    text[0x200..0x230].fill(0);
    assert!(evaluators(&text, BASE, &starts, &[DESCRIPTOR]).is_err());
}

#[test]
fn follows_new_experiment_dates_without_matching_other_phoenix_experiments() {
    let bytes = b"2026-11-phoenix-app-access2027-06-phoenix-app-access2027-06-phoenix-rollout";
    assert_eq!(experiment_offsets(bytes).collect::<Vec<_>>(), vec![0, 26]);
    assert!(experiment_offsets(b"garbage-phoenix-app-access").next().is_none());
}
