pub const MAX_DEPTH: u32 = 8;

pub struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
    failed: bool,
}

impl<'a> Cursor<'a> {
    pub fn new(bytes: &'a [u8], offset: usize) -> Self {
        Cursor {
            bytes,
            offset,
            failed: offset > bytes.len(),
        }
    }

    pub fn bytes(&self) -> &'a [u8] {
        self.bytes
    }

    pub fn failed(&self) -> bool {
        self.failed
    }

    pub fn offset(&self) -> usize {
        self.offset
    }

    pub fn fail(&mut self) {
        self.failed = true;
    }

    pub fn at_end(&self) -> bool {
        self.failed || self.offset >= self.bytes.len()
    }

    pub fn take(&mut self, count: usize) -> Option<&'a [u8]> {
        if self.failed || self.bytes.len() - self.offset < count {
            self.failed = true;
            return None;
        }
        let taken = &self.bytes[self.offset..self.offset + count];
        self.offset += count;
        Some(taken)
    }

    pub fn skip(&mut self, count: usize) {
        self.take(count);
    }

    pub fn read8(&mut self) -> u8 {
        self.take(1).map_or(0, |bytes| bytes[0])
    }

    pub fn read16(&mut self) -> u16 {
        self.take(2).map_or(0, |bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    pub fn read32(&mut self) -> u32 {
        self.take(4)
            .map_or(0, |bytes| u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    pub fn skip_value(&mut self, tag: u8, depth: u32) {
        if depth > MAX_DEPTH {
            self.failed = true;
            return;
        }
        match tag {
            0..=2 => {}
            3 => self.skip(4),
            4 | 5 => self.skip(8),
            6 => {
                let length = self.read32() as usize;
                self.skip(length);
            }
            7 => {
                let count = self.read32();
                for _ in 0..count {
                    if self.failed {
                        break;
                    }
                    self.skip_tagged(depth + 1);
                }
            }
            8 => {
                let count = self.read32();
                for _ in 0..count {
                    if self.failed {
                        break;
                    }
                    self.skip(2);
                    self.skip_tagged(depth + 1);
                }
            }
            9 => self.skip(16),
            10 => {
                let count = self.read32();
                for _ in 0..count {
                    if self.failed {
                        break;
                    }
                    let length = self.read32() as usize;
                    self.skip(length);
                    self.skip_tagged(depth + 1);
                }
            }
            11 => {
                let count = self.read32() as usize;
                self.skip(count.saturating_mul(4));
            }
            12 => {
                self.skip(4);
                let count = self.read16() as usize;
                self.skip(count * 6);
            }
            _ => self.failed = true,
        }
    }

    pub fn skip_tagged(&mut self, depth: u32) {
        let tag = self.read8();
        self.skip_value(tag, depth);
    }

    pub fn skip_properties(&mut self) {
        let count = self.read16();
        for _ in 0..count {
            if self.failed {
                break;
            }
            self.skip(2);
            self.skip_tagged(0);
        }
    }
}
