use std::sync::OnceLock;

const EVENT_FRAME: u8 = 1;
const NODE_OFFSET: usize = 2;
const EVENT_OFFSET: usize = 6;
const PAYLOAD_OFFSET: usize = EVENT_OFFSET + 2;
const OBJECT_TAG: u8 = 8;
const INT_TAG: u8 = 3;
const OBJECT_HEADER: usize = 5;
const INT_FIELD: usize = 7;
const NO_ROW: i64 = -1;

pub const PASS: i32 = 0;
pub const HANDLED: i32 = 1;
pub const HANDLED_CLOSE_MENU: i32 = 2;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EventIds {
    pub press: Option<u16>,
    pub change: Option<u16>,
    pub row_press_down: Option<u16>,
    pub index: Option<u16>,
}

static IDS: OnceLock<EventIds> = OnceLock::new();

fn position(names: &[String], wanted: &str) -> Option<u16> {
    names
        .iter()
        .position(|name| name == wanted)
        .and_then(|index| u16::try_from(index).ok())
}

pub fn ids_from(names: &[String]) -> EventIds {
    EventIds {
        press: position(names, "press"),
        change: position(names, "change"),
        row_press_down: position(names, "rowPressDown"),
        index: position(names, "index"),
    }
}

pub fn remember(names: &[String]) {
    let _ = IDS.set(ids_from(names));
}

fn u16_at(event: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(event.get(offset..offset + 2)?.try_into().ok()?))
}

fn i32_at(event: &[u8], offset: usize) -> Option<i32> {
    Some(i32::from_le_bytes(event.get(offset..offset + 4)?.try_into().ok()?))
}

pub fn row_index(event: &[u8], index_property: Option<u16>) -> i64 {
    let Some(index_property) = index_property else { return NO_ROW };
    if event.get(PAYLOAD_OFFSET) != Some(&OBJECT_TAG) {
        return NO_ROW;
    }
    let Some(fields) = i32_at(event, PAYLOAD_OFFSET + 1) else {
        return NO_ROW;
    };
    let mut offset = PAYLOAD_OFFSET + OBJECT_HEADER;
    for _ in 0..fields.max(0) {
        let (Some(property), Some(&INT_TAG)) = (u16_at(event, offset), event.get(offset + 2)) else {
            return NO_ROW;
        };
        if property == index_property {
            return i32_at(event, offset + 3).map_or(NO_ROW, i64::from);
        }
        offset += INT_FIELD;
    }
    NO_ROW
}

#[derive(Debug, PartialEq)]
pub enum Event<'a> {
    Press(i32),
    Change(i32, &'a [u8]),
    RowPressDown(i32, i64),
    Other,
}

pub fn decode<'a>(event: &'a [u8], ids: &EventIds) -> Event<'a> {
    if event.len() < PAYLOAD_OFFSET || event[0] != EVENT_FRAME || event[1] != EVENT_FRAME {
        return Event::Other;
    }
    let (Some(kind), Some(node)) = (u16_at(event, EVENT_OFFSET), i32_at(event, NODE_OFFSET)) else {
        return Event::Other;
    };
    match Some(kind) {
        kind if kind == ids.press => Event::Press(node),
        kind if kind == ids.change => Event::Change(node, &event[PAYLOAD_OFFSET..]),
        kind if kind == ids.row_press_down => Event::RowPressDown(node, row_index(event, ids.index)),
        _ => Event::Other,
    }
}

fn press(node: i32) -> i32 {
    if super::press(node) || super::press_inserted_node(node) {
        return HANDLED;
    }
    if super::press_message_action(node) {
        return HANDLED_CLOSE_MENU;
    }
    super::touch_message(node, NO_ROW);
    PASS
}

pub fn handle(event: &[u8]) -> i32 {
    let Some(ids) = IDS.get() else { return PASS };
    match decode(event, ids) {
        Event::Press(node) => press(node),
        Event::Change(node, payload) => {
            if super::slide(node, payload) {
                HANDLED
            } else {
                PASS
            }
        }
        Event::RowPressDown(node, row) => {
            super::touch_message(node, row);
            PASS
        }
        Event::Other => PASS,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids() -> EventIds {
        ids_from(&["x".into(), "press".into(), "change".into(), "rowPressDown".into(), "index".into()])
    }

    fn frame(kind: u16, node: i32, payload: &[u8]) -> Vec<u8> {
        let mut event = vec![EVENT_FRAME, EVENT_FRAME];
        event.extend_from_slice(&node.to_le_bytes());
        event.extend_from_slice(&kind.to_le_bytes());
        event.extend_from_slice(payload);
        event
    }

    #[test]
    fn decodes_presses_and_changes() {
        assert_eq!(decode(&frame(1, 7, &[]), &ids()), Event::Press(7));
        assert_eq!(decode(&frame(2, 9, &[4, 5]), &ids()), Event::Change(9, &[4, 5]));
        assert_eq!(decode(&frame(42, 9, &[]), &ids()), Event::Other);
        assert_eq!(decode(&[0, 1, 2], &ids()), Event::Other);
    }

    #[test]
    fn reads_the_pressed_row_index() {
        let mut payload = vec![OBJECT_TAG];
        payload.extend_from_slice(&2i32.to_le_bytes());
        payload.extend_from_slice(&0u16.to_le_bytes());
        payload.push(INT_TAG);
        payload.extend_from_slice(&5i32.to_le_bytes());
        payload.extend_from_slice(&4u16.to_le_bytes());
        payload.push(INT_TAG);
        payload.extend_from_slice(&31i32.to_le_bytes());
        assert_eq!(decode(&frame(3, 2, &payload), &ids()), Event::RowPressDown(2, 31));
        assert_eq!(row_index(&frame(3, 2, &[1]), Some(4)), NO_ROW);
    }

    #[test]
    fn missing_names_never_match() {
        let none = ids_from(&[]);
        assert_eq!(decode(&frame(0, 1, &[]), &none), Event::Other);
    }
}
