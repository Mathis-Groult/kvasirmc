//! A record of `SPlayerAuthInput::read` (`tests/golden/player_auth_input.txt`; rewrite it on
//! purpose with `KVASIR_BLESS=1`), made before it was split.
//!
//! One line per byte string: the decoded packet (Debug) and the bytes left over, or the error
//! (kind, message) and how many reads succeeded before it. The body is a pure decoder: each of the
//! five optional sections (item interaction, item stack request, block actions, vehicle rotation,
//! vehicle id) is gated by its own bool, never by the input flags, so the scenes cover all 32
//! present/absent combinations, the variants of each section, the flag-list limits and a sweep of
//! every truncation of one full packet.

use std::fmt::Write as _;
use std::io::{Error, Read};

use super::SPlayerAuthInput;
use crate::bedrock::network_item::{ContainerName, FullContainerName};
use crate::bedrock::server::item_stack_request::ItemStackRequestSlotInfo;
use crate::codec::{var_int::VarInt, var_uint::VarUInt, var_ulong::VarULong};
use crate::serial::{PacketRead, PacketWrite};

type TestResult = Result<(), Box<dyn std::error::Error>>;
type Bytes = Result<Vec<u8>, Error>;

/// Debug text longer than this is recorded as its length and a hash.
const LONG_TEXT: usize = 4000;

#[allow(clippy::needless_pass_by_value)]
fn put<T: PacketWrite>(out: &mut Vec<u8>, value: T) -> Result<(), Error> {
    value.write(out)
}

/// `count` distinct floats starting at index `first`, so a swapped field shows in the Debug text.
fn floats(out: &mut Vec<u8>, first: u16, count: u16) -> Result<(), Error> {
    for i in first..first + count {
        put(out, f32::from(i) * 0.5 + 0.25)?;
    }
    Ok(())
}

/// Everything before the optional sections; `flags` are written after a flag count of `count`.
fn head(count: u32, flags: &[i32]) -> Bytes {
    let mut out = Vec::new();
    floats(&mut out, 1, 8)?;
    put(&mut out, VarUInt(count))?;
    for flag in flags {
        put(&mut out, VarInt(*flag))?;
    }
    put(&mut out, VarUInt(2))?;
    put(&mut out, VarUInt(3))?;
    put(&mut out, VarInt(-4))?;
    floats(&mut out, 20, 2)?;
    put(&mut out, VarULong(300))?;
    floats(&mut out, 30, 3)?;
    Ok(out)
}

fn descriptor(out: &mut Vec<u8>, id: i16, size: u16, net_id: Option<i32>) -> Result<(), Error> {
    put(out, id)?;
    put(out, size)?;
    put(out, VarUInt(9))?;
    put(out, net_id.is_some())?;
    if let Some(net_id) = net_id {
        put(out, VarInt(net_id))?;
    }
    put(out, VarUInt(5))?;
    put(out, VarUInt(0))
}

fn use_item_transaction(out: &mut Vec<u8>) -> Result<(), Error> {
    put(out, VarInt(3))?;
    put(out, 1u8)?;
    for coordinate in [10, 64, -5] {
        put(out, VarInt(coordinate))?;
    }
    put(out, 2u8)?;
    put(out, VarInt(4))?;
    descriptor(out, 5, 7, None)?;
    floats(out, 50, 3)?;
    floats(out, 60, 3)?;
    put(out, VarUInt(11))?;
    put(out, 1u8)?;
    put(out, 2u8)
}

fn inventory_action(out: &mut Vec<u8>, i: u32) -> Result<(), Error> {
    put(out, VarUInt(i % 4))?;
    let window = i.is_multiple_of(2);
    put(out, window)?;
    if window {
        put(out, -1i8)?;
    }
    let flags = i.is_multiple_of(3);
    put(out, flags)?;
    if flags {
        put(out, VarUInt(8))?;
    }
    put(out, VarUInt(i))?;
    descriptor(out, 1, 2, Some(3))?;
    descriptor(out, 4, 0, None)
}

/// An item interaction: `slots` is the legacy slot group count written when the legacy block is
/// present, `actions` the action count written and `real` the actions that follow it.
fn item(legacy_id: i32, flag: bool, slots: Option<u32>, actions: u32, real: u32) -> Bytes {
    let mut out = Vec::new();
    put(&mut out, VarInt(legacy_id))?;
    put(&mut out, flag)?;
    if let Some(groups) = slots {
        put(&mut out, VarUInt(groups))?;
        for group in 0..groups {
            put(&mut out, 3 + u8::try_from(group % 100).unwrap_or(0))?;
            put(&mut out, VarUInt(2))?;
            put(&mut out, 6u8)?;
            put(&mut out, 7u8)?;
        }
    }
    put(&mut out, VarUInt(actions))?;
    for i in 0..real {
        inventory_action(&mut out, i)?;
    }
    use_item_transaction(&mut out)?;
    Ok(out)
}

fn slot_info(out: &mut Vec<u8>, slot: u8) -> Result<(), Error> {
    ItemStackRequestSlotInfo {
        container_name: FullContainerName {
            container_name: ContainerName::Inventory,
            dynamic_id: Some(9),
        },
        slot_id: slot,
        stack_id: -2,
    }
    .write(out)
}

/// An item stack request: `kind` 0 = empty, 1 = create + take actions and two filters,
/// 2 = an unknown action id.
fn stack_request(kind: u8) -> Bytes {
    let mut out = Vec::new();
    put(&mut out, VarInt(12))?;
    match kind {
        0 => {
            put(&mut out, VarUInt(0))?;
            put(&mut out, VarUInt(0))?;
        }
        1 => {
            put(&mut out, VarUInt(2))?;
            put(&mut out, VarUInt(6))?;
            put(&mut out, 6u8)?;
            put(&mut out, 7u8)?;
            put(&mut out, VarUInt(0))?;
            put(&mut out, 0u8)?;
            put(&mut out, 3u8)?;
            slot_info(&mut out, 1)?;
            slot_info(&mut out, 2)?;
            put(&mut out, VarUInt(2))?;
            put(&mut out, "ab".to_string())?;
            put(&mut out, "cde".to_string())?;
        }
        _ => {
            put(&mut out, VarUInt(1))?;
            put(&mut out, VarUInt(99))?;
            put(&mut out, 0u8)?;
        }
    }
    put(&mut out, 5i32)?;
    Ok(out)
}

/// Block actions: `count` is the count written, `real` the actions that follow it.
fn block_actions(count: u32, real: u32) -> Bytes {
    let mut out = Vec::new();
    put(&mut out, VarUInt(count))?;
    for i in 0..real {
        put(&mut out, VarInt((i % 7) as i32))?;
        for coordinate in [i as i32, 70, -(i as i32)] {
            put(&mut out, VarInt(coordinate))?;
        }
        put(&mut out, VarInt((i % 6) as i32))?;
    }
    Ok(out)
}

struct Sections {
    item: Option<Vec<u8>>,
    stack_request: Option<Vec<u8>>,
    block_actions: Option<Vec<u8>>,
    vehicle_rotation: bool,
    vehicle_id: bool,
}

impl Sections {
    const fn none() -> Self {
        Self {
            item: None,
            stack_request: None,
            block_actions: None,
            vehicle_rotation: false,
            vehicle_id: false,
        }
    }
}

/// Head, then each optional section behind its bool, then the trailing seven floats.
fn assemble(mut out: Vec<u8>, sections: &Sections) -> Bytes {
    for body in [
        &sections.item,
        &sections.stack_request,
        &sections.block_actions,
    ] {
        put(&mut out, body.is_some())?;
        if let Some(body) = body {
            out.extend_from_slice(body);
        }
    }
    put(&mut out, sections.vehicle_rotation)?;
    if sections.vehicle_rotation {
        floats(&mut out, 70, 2)?;
    }
    put(&mut out, sections.vehicle_id)?;
    if sections.vehicle_id {
        put(&mut out, VarInt(-123_456))?;
    }
    floats(&mut out, 80, 7)?;
    Ok(out)
}

/// Counts the reads that returned data, so an error line says how far the decoder got.
struct Trace<'a> {
    rest: &'a [u8],
    reads: usize,
}

impl Read for Trace<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.rest.read(buf)?;
        if n > 0 {
            self.reads += 1;
        }
        Ok(n)
    }
}

fn fnv32(text: &str) -> u32 {
    text.bytes().fold(0x811c_9dc5, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
    })
}

fn render(bytes: &[u8]) -> String {
    let mut reader = Trace {
        rest: bytes,
        reads: 0,
    };
    match SPlayerAuthInput::read(&mut reader) {
        Ok(packet) => {
            let text = format!("{packet:?}");
            let rest = reader.rest.len();
            if text.len() > LONG_TEXT {
                format!(
                    "ok {} chars fnv {:08x} rest {rest}",
                    text.len(),
                    fnv32(&text)
                )
            } else {
                format!("ok {text} rest {rest}")
            }
        }
        Err(error) => format!(
            "err {:?} {error} after {} reads",
            error.kind(),
            reader.reads
        ),
    }
}

fn flag_scenes() -> Result<Vec<(String, Vec<u8>)>, Error> {
    let all: Vec<i32> = (0..66).collect();
    let cases: Vec<(&str, u32, Vec<i32>)> = vec![
        ("no flags", 0, vec![]),
        ("flag 0", 1, vec![0]),
        ("flag 65", 1, vec![65]),
        ("flags 5 and 40", 2, vec![5, 40]),
        ("flags 64 and 1", 2, vec![64, 1]),
        ("all 66 flags", 66, all),
        ("67 flags", 67, vec![]),
        ("68 flags", 68, vec![]),
        ("flag 66", 1, vec![66]),
        ("flag 67", 1, vec![67]),
        ("flag -1", 1, vec![-1]),
        ("flag -66", 1, vec![-66]),
        ("duplicate flag", 2, vec![9, 9]),
        ("duplicate after another", 3, vec![9, 30, 9]),
        ("duplicate 65", 2, vec![65, 65]),
        ("count 2, one flag", 2, vec![4]),
    ];
    let mut scenes = Vec::new();
    for (name, count, flags) in cases {
        let bytes = assemble(head(count, &flags)?, &Sections::none())?;
        scenes.push((format!("{name}, no sections"), bytes));
    }
    Ok(scenes)
}

fn section_combinations() -> Result<Vec<(String, Vec<u8>)>, Error> {
    let mut scenes = Vec::new();
    for mask in 0u8..32 {
        let on = |bit: u8| mask & (1 << bit) != 0;
        let sections = Sections {
            item: if on(0) {
                Some(item(-2, true, Some(2), 2, 2)?)
            } else {
                None
            },
            stack_request: if on(1) { Some(stack_request(1)?) } else { None },
            block_actions: if on(2) {
                Some(block_actions(3, 3)?)
            } else {
                None
            },
            vehicle_rotation: on(3),
            vehicle_id: on(4),
        };
        let bytes = assemble(head(4, &[34, 35, 36, 45])?, &sections)?;
        scenes.push((
            format!(
                "sections item={} request={} blocks={} rot={} id={}",
                u8::from(on(0)),
                u8::from(on(1)),
                u8::from(on(2)),
                u8::from(on(3)),
                u8::from(on(4)),
            ),
            bytes,
        ));
    }
    Ok(scenes)
}

fn with_item(name: &str, body: Vec<u8>) -> Result<(String, Vec<u8>), Error> {
    let sections = Sections {
        item: Some(body),
        ..Sections::none()
    };
    Ok((
        format!("item: {name}"),
        assemble(head(1, &[34])?, &sections)?,
    ))
}

fn item_scenes() -> Result<Vec<(String, Vec<u8>)>, Error> {
    Ok(vec![
        with_item("id 0, no flag, no actions", item(0, false, None, 0, 0)?)?,
        with_item(
            "id -2, flag, two slot groups",
            item(-2, true, Some(2), 0, 0)?,
        )?,
        with_item("id -2, flag, no slot group", item(-2, true, Some(0), 1, 1)?)?,
        with_item("id -2, no flag", item(-2, false, None, 1, 1)?)?,
        with_item(
            "id -4, flag, one slot group",
            item(-4, true, Some(1), 2, 2)?,
        )?,
        with_item("id -3 (odd), flag", item(-3, true, None, 1, 1)?)?,
        with_item("id -5 (odd), flag", item(-5, true, None, 0, 0)?)?,
        with_item("id -1, flag", item(-1, true, None, 0, 0)?)?,
        with_item("id 0, flag", item(0, true, None, 0, 0)?)?,
        with_item("id 2 (even, positive), flag", item(2, true, None, 0, 0)?)?,
        with_item("1024 slot groups", item(-2, true, Some(1024), 0, 0)?)?,
        with_item("1025 slot groups", item(-2, true, Some(1025), 0, 0)?)?,
        with_item("1024 actions", item(0, false, None, 1024, 1024)?)?,
        with_item("1025 actions", item(0, false, None, 1025, 0)?)?,
        with_item("six actions", item(0, false, None, 6, 6)?)?,
        with_item(
            "action count over what follows",
            item(0, false, None, 3, 1)?,
        )?,
    ])
}

fn with_request(name: &str, kind: u8) -> Result<(String, Vec<u8>), Error> {
    let sections = Sections {
        stack_request: Some(stack_request(kind)?),
        ..Sections::none()
    };
    Ok((
        format!("request: {name}"),
        assemble(head(1, &[36])?, &sections)?,
    ))
}

fn with_blocks(name: &str, count: u32, real: u32) -> Result<(String, Vec<u8>), Error> {
    let sections = Sections {
        block_actions: Some(block_actions(count, real)?),
        ..Sections::none()
    };
    Ok((
        format!("blocks: {name}"),
        assemble(head(1, &[35])?, &sections)?,
    ))
}

fn with_vehicle(name: &str, rotation: bool, id: bool) -> Result<(String, Vec<u8>), Error> {
    let sections = Sections {
        vehicle_rotation: rotation,
        vehicle_id: id,
        ..Sections::none()
    };
    Ok((
        format!("vehicle: {name}"),
        assemble(head(1, &[45])?, &sections)?,
    ))
}

fn section_scenes() -> Result<Vec<(String, Vec<u8>)>, Error> {
    let mut scenes = item_scenes()?;
    scenes.push(with_request("empty", 0)?);
    scenes.push(with_request("create, take, two filters", 1)?);
    scenes.push(with_request("unknown action", 2)?);
    scenes.push(with_blocks("none", 0, 0)?);
    scenes.push(with_blocks("one", 1, 1)?);
    scenes.push(with_blocks("seven", 7, 7)?);
    scenes.push(with_blocks("64", 64, 64)?);
    scenes.push(with_blocks("65", 65, 65)?);
    scenes.push(with_blocks("1024", 1024, 1024)?);
    scenes.push(with_blocks("1025", 1025, 0)?);
    scenes.push(with_blocks("count over what follows", 3, 2)?);
    scenes.push(with_vehicle("rotation only", true, false)?);
    scenes.push(with_vehicle("id only", false, true)?);
    scenes.push(with_vehicle("both", true, true)?);
    Ok(scenes)
}

fn full_packet() -> Bytes {
    let sections = Sections {
        item: Some(item(-2, true, Some(1), 1, 1)?),
        stack_request: Some(stack_request(1)?),
        block_actions: Some(block_actions(2, 2)?),
        vehicle_rotation: true,
        vehicle_id: true,
    };
    assemble(head(2, &[34, 45])?, &sections)
}

/// Every truncation of the full packet, runs of equal results folded into one line.
fn truncation_lines(full: &[u8]) -> Result<String, std::fmt::Error> {
    let mut text = String::new();
    let mut run: Option<(usize, String)> = None;
    for len in 0..=full.len() {
        let result = render(&full[..len]);
        match &run {
            Some((_, previous)) if *previous == result => {}
            _ => {
                if let Some((start, previous)) = run.take() {
                    writeln!(text, "truncated {start}..{}: {previous}", len - 1)?;
                }
                run = Some((len, result));
            }
        }
    }
    if let Some((start, previous)) = run {
        writeln!(text, "truncated {start}..{}: {previous}", full.len())?;
    }
    Ok(text)
}

fn record() -> Result<String, Box<dyn std::error::Error>> {
    let mut scenes = flag_scenes()?;
    scenes.extend(section_combinations()?);
    scenes.extend(section_scenes()?);
    let full = full_packet()?;
    let mut longer = full.clone();
    longer.push(0xEE);
    scenes.push(("full packet".to_string(), full.clone()));
    scenes.push(("full packet and one byte more".to_string(), longer));

    let mut now = String::new();
    for (name, bytes) in &scenes {
        writeln!(now, "{name} ({} bytes): {}", bytes.len(), render(bytes))?;
    }
    now.push_str(&truncation_lines(&full)?);
    Ok(now)
}

#[test]
fn auth_input_decodes_the_same() -> TestResult {
    let now = record()?;
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join("player_auth_input.txt");
    if std::env::var_os("KVASIR_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().ok_or("no parent")?)?;
        std::fs::write(&path, now)?;
    } else {
        assert_eq!(std::fs::read_to_string(&path)?, now);
    }
    Ok(())
}
