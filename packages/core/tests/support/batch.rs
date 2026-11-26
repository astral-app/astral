const CREATE: u8 = 1;
const SET_ROOT: u8 = 2;
const SET_CHILDREN: u8 = 3;
const STRING: u8 = 6;
const ARRAY: u8 = 7;
const INTEGER_ARRAY: u8 = 11;
const DIMENSION: u8 = 12;
const IDENTIFIER: u16 = 9;
const LABEL: u16 = 11;
const DIRECTION: u16 = 147;
const TEXT: u16 = 499;
const SOURCE: u16 = 542;
const PRESS: u16 = 320;
const EVENTS: u16 = 733;
const BACK_WIDTH: u16 = 740;
const ROOT_KEY: u16 = 252;

#[derive(Default)]
pub struct Batch(pub Vec<u8>);

impl Batch {
    fn byte(&mut self, value: u8) {
        self.0.push(value);
    }

    fn short(&mut self, value: u16) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }

    fn int(&mut self, value: i32) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }

    fn text(&mut self, value: &str) {
        self.int(i32::try_from(value.len()).expect("length"));
        self.0.extend_from_slice(value.as_bytes());
    }

    fn create(&mut self, id: i32, kind: &str, properties: u16) {
        self.byte(CREATE);
        self.int(id);
        self.short(u16::try_from(kind.len()).expect("kind"));
        self.0.extend_from_slice(kind.as_bytes());
        self.short(properties);
    }

    fn string(&mut self, property: u16, value: &str) {
        self.short(property);
        self.byte(STRING);
        self.text(value);
    }

    fn children(&mut self, parent: i32, ids: &[i32]) {
        self.byte(SET_CHILDREN);
        self.int(parent);
        self.int(i32::try_from(ids.len()).expect("count"));
        for id in ids {
            self.int(*id);
        }
    }

    fn skeleton(&mut self) {
        self.create(5, "navigator", 0);
        self.create(50, "navigator", 0);
        self.create(51, "view", 0);
        self.create(6, "view", 0);
        for id in [100, 101, 102, 103, 104, 105, 106, 107, 108, 109, 110, 112, 113, 115, 116] {
            self.create(id, "view", 0);
        }
        self.create(111, "view", 2);
        self.string(IDENTIFIER, "Back");
        self.short(BACK_WIDTH);
        self.byte(DIMENSION);
        self.int(0);
        self.short(0);
        self.create(114, "image", 1);
        self.string(SOURCE, "res://ic_mana_x_small");
        self.create(117, "text", 1);
        self.string(TEXT, "Settings");
        self.create(30, "view", 0);
        self.create(31, "view", 0);
    }

    fn links(&mut self) {
        self.children(30, &[31]);
        self.children(100, &[101]);
        self.children(101, &[102]);
        self.children(102, &[103]);
        self.children(103, &[104, 105, 106, 107]);
        self.children(106, &[10]);
        self.children(107, &[108, 115]);
        self.children(108, &[109]);
        self.children(109, &[110]);
        self.children(110, &[111]);
        self.children(111, &[112]);
        self.children(112, &[113]);
        self.children(113, &[114]);
        self.children(115, &[116]);
        self.children(116, &[117]);
        self.children(5, &[6, 100]);
        self.children(50, &[51]);
        self.byte(SET_ROOT);
        self.int(5);
        self.short(1);
        self.short(ROOT_KEY);
        self.byte(SET_CHILDREN);
        self.int(1);
    }

    pub fn overview() -> Batch {
        let mut batch = Batch::default();
        batch.byte(1);
        batch.skeleton();
        batch.create(10, "view", 0);
        batch.create(11, "input", 0);
        batch.create(20, "view", 1);
        batch.string(DIRECTION, "column");
        batch.create(21, "text", 1);
        batch.string(TEXT, "Account Settings");
        batch.create(22, "view", 0);
        batch.create(23, "view", 4);
        batch.string(IDENTIFIER, "settings.row.account");
        batch.string(LABEL, "Account");
        batch.short(PRESS);
        batch.byte(ARRAY);
        batch.int(1);
        batch.byte(STRING);
        batch.text("press");
        batch.short(EVENTS);
        batch.byte(INTEGER_ARRAY);
        batch.int(2);
        batch.int(7);
        batch.int(9);
        batch.create(24, "view", 0);
        batch.create(25, "image", 1);
        batch.string(SOURCE, "res://ic_mana_user_circle");
        batch.create(26, "text", 1);
        batch.string(TEXT, "Account");
        batch.create(27, "view", 0);
        batch.create(28, "image", 0);
        batch.children(27, &[28]);
        batch.children(24, &[25, 26, 27]);
        batch.children(23, &[24]);
        batch.children(22, &[23, 30]);
        batch.children(20, &[21, 22]);
        batch.children(10, &[11, 20]);
        batch.links();
        batch
    }
}
