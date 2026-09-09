use crate::monitor::{Direction, Event, TimedEvent};
use crate::{LorgarError, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, Read, Seek, SeekFrom, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

const MAGIC: &[u8; 8] = b"LORGAR\x01\n";
const MAX_RECORD: usize = 128 * 1024;
const MAX_CAPTURE: u64 = 256 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Descriptor {
    pub device: crate::LorgarDeviceInfo,
    pub raw: Vec<u8>,
    #[serde(default)]
    pub layouts: Vec<crate::descriptor::ReportLayout>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Header {
    pub version: u32,
    pub created_ms: u64,
    pub source: String,
    pub descriptors: Vec<Descriptor>,
}

pub struct CaptureWriter {
    file: File,
}

fn write_record(file: &mut File, value: &impl Serialize) -> Result<()> {
    let bytes = serde_json::to_vec(value)?;
    if bytes.len() > MAX_RECORD {
        return Err(LorgarError::Config("capture record exceeds limit".into()));
    }
    file.write_all(&(bytes.len() as u32).to_le_bytes())?;
    file.write_all(&bytes)?;
    Ok(())
}

impl CaptureWriter {
    pub fn create(path: &Path, header: &Header) -> Result<Self> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(MAGIC)?;
        write_record(&mut file, header)?;
        file.sync_data()?;
        Ok(Self { file })
    }
    pub fn push(&mut self, event: &TimedEvent) -> Result<()> {
        write_record(&mut self.file, event)?;
        self.file.sync_data()?;
        Ok(())
    }
}

fn read_record(reader: &mut impl Read) -> Result<Option<Vec<u8>>> {
    let mut length = [0; 4];
    if reader.read(&mut length[..1])? == 0 {
        return Ok(None);
    }
    reader.read_exact(&mut length[1..])?;
    let length = u32::from_le_bytes(length) as usize;
    if length > MAX_RECORD {
        return Err(LorgarError::Config("capture record exceeds limit".into()));
    }
    let mut bytes = vec![0; length];
    reader.read_exact(&mut bytes)?;
    Ok(Some(bytes))
}

pub fn read_capture(path: &Path) -> Result<(Header, Vec<TimedEvent>)> {
    let mut file = File::open(path)?;
    if file.metadata()?.len() > MAX_CAPTURE {
        return Err(LorgarError::Config(
            "capture exceeds 256 MiB analysis limit; use shorter captures".into(),
        ));
    }
    let mut magic = [0; 8];
    file.read_exact(&mut magic)?;
    if magic[0] == b'{' {
        file.seek(SeekFrom::Start(0))?;
        let mut lines = std::io::BufReader::new(file).lines();
        let first = lines
            .next()
            .ok_or_else(|| LorgarError::Config("missing JSON capture header".into()))??;
        if first.len() > MAX_RECORD {
            return Err(LorgarError::Config("capture header exceeds limit".into()));
        }
        let value: serde_json::Value = serde_json::from_str(&first)?;
        if value["type"] != "capture_header" {
            return Err(LorgarError::Config(
                "not a lorgar JSON capture export".into(),
            ));
        }
        let header: Header = serde_json::from_value(value["header"].clone())?;
        if header.version != 1 {
            return Err(LorgarError::Config("unsupported capture version".into()));
        }
        let mut events = Vec::new();
        for line in lines {
            let line = line?;
            if line.len() > MAX_RECORD {
                return Err(LorgarError::Config("capture record exceeds limit".into()));
            }
            events.push(serde_json::from_str(&line)?);
        }
        return Ok((header, events));
    }
    if magic != *MAGIC {
        return Err(LorgarError::Config("not a supported lorgar capture".into()));
    }
    let header: Header = serde_json::from_slice(
        &read_record(&mut file)?
            .ok_or_else(|| LorgarError::Config("missing capture header".into()))?,
    )?;
    if header.version != 1 {
        return Err(LorgarError::Config("unsupported capture version".into()));
    }
    let mut events = Vec::new();
    while let Some(bytes) = read_record(&mut file)? {
        events.push(serde_json::from_slice(&bytes)?);
    }
    Ok((header, events))
}

pub fn descriptors() -> Result<Vec<Descriptor>> {
    let mut descriptors = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for device in crate::Msa10w::list_devices()? {
        if !seen.insert(device.path.clone()) {
            continue;
        }
        let Some(name) = Path::new(&device.path).file_name() else {
            continue;
        };
        let path = Path::new("/sys/class/hidraw")
            .join(name)
            .join("device/report_descriptor");
        match std::fs::read(path) {
            Ok(raw) => {
                let layouts = crate::descriptor::layouts(&raw)?;
                descriptors.push(Descriptor {
                    device,
                    raw,
                    layouts,
                });
            }
            Err(e) => log::warn!("cannot read descriptor for {}: {e}", device.path),
        }
    }
    Ok(descriptors)
}

#[derive(Debug, Serialize)]
pub struct ByteChange {
    pub packet_index: usize,
    pub offset: usize,
    pub before: u8,
    pub after: u8,
}

#[derive(Debug, Serialize)]
pub struct Pattern {
    pub raw: Vec<u8>,
    pub count: usize,
}

#[derive(Debug, Serialize)]
pub struct StreamDiff {
    pub stream: String,
    pub changes: Vec<ByteChange>,
    pub only_before: Vec<Pattern>,
    pub only_after: Vec<Pattern>,
    pub repeated_before: Vec<Pattern>,
    pub repeated_after: Vec<Pattern>,
    pub candidate_fields: Vec<String>,
}

fn groups(events: &[TimedEvent]) -> BTreeMap<String, Vec<Vec<u8>>> {
    let mut groups: BTreeMap<String, Vec<Vec<u8>>> = BTreeMap::new();
    for event in events {
        if let Event::Packet {
            device,
            direction,
            raw,
            ..
        } = &event.event
        {
            let report = if device.is_config_interface && *direction == Direction::Input {
                "unnumbered".into()
            } else {
                raw.first()
                    .map(|b| format!("{b:02x}"))
                    .unwrap_or_else(|| "none".into())
            };
            let key = format!(
                "{:04x}:{:04x}/interface={}/page={:04x}/{direction:?}/report={}/len={}",
                device.vendor_id,
                device.product_id,
                device.interface_number,
                device.usage_page,
                report,
                raw.len()
            );
            groups.entry(key).or_default().push(raw.clone());
        }
    }
    groups
}

fn counts(packets: &[Vec<u8>]) -> BTreeMap<Vec<u8>, usize> {
    let mut counts = BTreeMap::new();
    for packet in packets {
        *counts.entry(packet.clone()).or_default() += 1;
    }
    counts
}

fn excess(left: &BTreeMap<Vec<u8>, usize>, right: &BTreeMap<Vec<u8>, usize>) -> Vec<Pattern> {
    left.iter()
        .filter_map(|(raw, count)| {
            let count = count.saturating_sub(*right.get(raw).unwrap_or(&0));
            (count > 0).then(|| Pattern {
                raw: raw.clone(),
                count,
            })
        })
        .collect()
}

fn candidates(packets: &[Vec<u8>]) -> Vec<String> {
    if packets.len() < 4 {
        return Vec::new();
    }
    let len = packets[0].len();
    if packets.iter().any(|p| p.len() != len) {
        return Vec::new();
    }
    let mut fields = Vec::new();
    for offset in 1..len {
        if packets.iter().all(|p| p[offset] == packets[0][offset]) {
            continue;
        }
        if packets
            .windows(2)
            .all(|p| p[1][offset] == p[0][offset].wrapping_add(1))
        {
            fields.push(format!(
                "offset {offset}: candidate modulo-256 counter (+1 in {} samples), unconfirmed",
                packets.len()
            ));
        }
        for (name, xor) in [("sum8", false), ("xor8", true)] {
            if packets.iter().all(|p| {
                p.iter()
                    .enumerate()
                    .filter(|(i, _)| *i != offset)
                    .fold(0u8, |sum, (_, byte)| {
                        if xor {
                            sum ^ byte
                        } else {
                            sum.wrapping_add(*byte)
                        }
                    })
                    == p[offset]
            }) {
                fields.push(format!("offset {offset}: candidate {name} of all other bytes including report ID, unconfirmed"));
            }
        }
    }
    fields
}

pub fn diff(before: &[TimedEvent], after: &[TimedEvent]) -> Vec<StreamDiff> {
    let before = groups(before);
    let after = groups(after);
    let keys: std::collections::BTreeSet<_> = before.keys().chain(after.keys()).collect();
    keys.into_iter()
        .map(|key| {
            let left = before.get(key).map(Vec::as_slice).unwrap_or_default();
            let right = after.get(key).map(Vec::as_slice).unwrap_or_default();
            let mut changes = Vec::new();
            for (packet_index, (a, b)) in left.iter().zip(right).enumerate() {
                for (offset, (&before, &after)) in a.iter().zip(b).enumerate() {
                    if before != after {
                        changes.push(ByteChange {
                            packet_index,
                            offset,
                            before,
                            after,
                        });
                    }
                }
            }
            let a = counts(left);
            let b = counts(right);
            let repeated = |counts: &BTreeMap<Vec<u8>, usize>| {
                counts
                    .iter()
                    .filter(|(_, c)| **c > 1)
                    .map(|(raw, count)| Pattern {
                        raw: raw.clone(),
                        count: *count,
                    })
                    .collect()
            };
            let mut candidate_fields = candidates(left)
                .into_iter()
                .map(|s| format!("before: {s}"))
                .collect::<Vec<_>>();
            candidate_fields.extend(candidates(right).into_iter().map(|s| format!("after: {s}")));
            StreamDiff {
                stream: key.clone(),
                changes,
                only_before: excess(&a, &b),
                only_after: excess(&b, &a),
                repeated_before: repeated(&a),
                repeated_after: repeated(&b),
                candidate_fields,
            }
        })
        .collect()
}

pub fn input_only(event: &TimedEvent) -> bool {
    matches!(
        event.event,
        Event::Packet {
            direction: Direction::Input,
            ..
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capture_roundtrip_and_truncation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("capture.bin");
        let header = Header {
            version: 1,
            created_ms: 1,
            source: "test".into(),
            descriptors: vec![],
        };
        let event = TimedEvent::new(Event::Warning {
            message: "test".into(),
        });
        let mut writer = CaptureWriter::create(&path, &header).unwrap();
        writer.push(&event).unwrap();
        assert!(CaptureWriter::create(&path, &header).is_err());
        let (_, events) = read_capture(&path).unwrap();
        assert_eq!(
            serde_json::to_value(&events[0]).unwrap(),
            serde_json::to_value(event).unwrap()
        );
        writer
            .file
            .set_len(writer.file.metadata().unwrap().len() - 1)
            .unwrap();
        assert!(read_capture(&path).is_err());
    }
    #[test]
    fn hypotheses_require_varying_repeated_evidence() {
        assert!(candidates(&vec![vec![5, 1]; 4]).is_empty());
        let packets = vec![vec![5, 1, 6], vec![5, 2, 7], vec![5, 3, 8], vec![5, 4, 9]];
        let result = candidates(&packets);
        assert!(result
            .iter()
            .any(|s| s.contains("offset 1: candidate modulo")));
        assert!(result
            .iter()
            .any(|s| s.contains("offset 2: candidate sum8")));
    }
}
