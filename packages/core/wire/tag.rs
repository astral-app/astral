#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Tag {
    Null = 0,
    False = 1,
    True = 2,
    Int = 3,
    Long = 4,
    Double = 5,
    String = 6,
    Array = 7,
    Map = 8,
    Quad = 9,
    Object = 10,
    Words = 11,
    WordRef = 12,
}

impl Tag {
    pub const fn parse(byte: u8) -> Option<Tag> {
        Some(match byte {
            0 => Tag::Null,
            1 => Tag::False,
            2 => Tag::True,
            3 => Tag::Int,
            4 => Tag::Long,
            5 => Tag::Double,
            6 => Tag::String,
            7 => Tag::Array,
            8 => Tag::Map,
            9 => Tag::Quad,
            10 => Tag::Object,
            11 => Tag::Words,
            12 => Tag::WordRef,
            _ => return None,
        })
    }

    pub const fn flag(value: bool) -> Tag {
        if value { Tag::True } else { Tag::False }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Op {
    Create = 1,
    Update = 2,
    SetChildren = 3,
    Remove = 4,
    SetRoot = 5,
    SetFocus = 6,
    Command = 7,
    Commit = 8,
    SharedValues = 9,
    EditChildren = 10,
}

impl Op {
    pub const fn parse(byte: u8) -> Option<Op> {
        Some(match byte {
            1 => Op::Create,
            2 => Op::Update,
            3 => Op::SetChildren,
            4 => Op::Remove,
            5 => Op::SetRoot,
            6 => Op::SetFocus,
            7 => Op::Command,
            8 => Op::Commit,
            9 => Op::SharedValues,
            10 => Op::EditChildren,
            _ => return None,
        })
    }
}

impl PartialEq<Tag> for u8 {
    fn eq(&self, other: &Tag) -> bool {
        *self == *other as u8
    }
}

impl PartialEq<Op> for u8 {
    fn eq(&self, other: &Op) -> bool {
        *self == *other as u8
    }
}
