use crate::{LorgarError, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportKind {
    Input,
    Output,
    Feature,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportLayout {
    pub report_id: Option<u8>,
    pub kind: ReportKind,
    pub payload_bits: u32,
    pub wire_bytes: u32,
    pub usage_pages: Vec<u32>,
}

#[derive(Clone, Default)]
struct Globals {
    size: u32,
    count: u32,
    id: Option<u8>,
    usage_page: u32,
}

pub fn layouts(raw: &[u8]) -> Result<Vec<ReportLayout>> {
    let invalid =
        || LorgarError::InvalidPacket("malformed or excessive HID report descriptor".into());
    let mut globals = Globals::default();
    let mut stack = Vec::new();
    let mut reports = BTreeMap::<(Option<u8>, ReportKind), ReportLayout>::new();
    let mut offset = 0;
    while offset < raw.len() {
        let prefix = raw[offset];
        offset += 1;
        if prefix == 0xfe {
            let length = *raw.get(offset).ok_or_else(invalid)? as usize;
            offset = offset.checked_add(2 + length).ok_or_else(invalid)?;
            if offset > raw.len() {
                return Err(invalid());
            }
            continue;
        }
        let size = match prefix & 3 {
            3 => 4,
            size => size as usize,
        };
        let data = raw.get(offset..offset + size).ok_or_else(invalid)?;
        offset += size;
        let value = data
            .iter()
            .enumerate()
            .fold(0u32, |value, (i, b)| value | ((*b as u32) << (8 * i)));
        let item_type = (prefix >> 2) & 3;
        let tag = prefix >> 4;
        if item_type == 1 {
            match tag {
                0 => globals.usage_page = value,
                7 => globals.size = value,
                8 if (1..=255).contains(&value) => globals.id = Some(value as u8),
                8 => return Err(invalid()),
                9 => globals.count = value,
                10 => {
                    if stack.len() >= 32 {
                        return Err(invalid());
                    }
                    stack.push(globals.clone());
                }
                11 => globals = stack.pop().ok_or_else(invalid)?,
                _ => (),
            }
        } else if item_type == 0 {
            let kind = match tag {
                8 => ReportKind::Input,
                9 => ReportKind::Output,
                11 => ReportKind::Feature,
                _ => continue,
            };
            let bits = globals
                .size
                .checked_mul(globals.count)
                .ok_or_else(invalid)?;
            let report = reports
                .entry((globals.id, kind))
                .or_insert_with(|| ReportLayout {
                    report_id: globals.id,
                    kind,
                    payload_bits: 0,
                    wire_bytes: 0,
                    usage_pages: Vec::new(),
                });
            report.payload_bits = report.payload_bits.checked_add(bits).ok_or_else(invalid)?;
            if report.payload_bits > 32768 {
                return Err(invalid());
            }
            report.wire_bytes =
                report.payload_bits.div_ceil(8) + u32::from(report.report_id.is_some());
            if !report.usage_pages.contains(&globals.usage_page) {
                report.usage_pages.push(globals.usage_page);
            }
        }
    }
    if !stack.is_empty() {
        return Err(invalid());
    }
    Ok(reports.into_values().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn observed_configuration_descriptor_is_unnumbered() {
        let raw = [
            6, 16, 255, 9, 1, 161, 1, 21, 0, 38, 255, 0, 117, 8, 149, 64, 9, 241, 129, 2, 9, 242,
            145, 0, 9, 243, 149, 8, 177, 0, 192,
        ];
        let reports = layouts(&raw).unwrap();
        assert!(reports.iter().all(|r| r.report_id.is_none()));
        assert_eq!(
            reports
                .iter()
                .find(|r| r.kind == ReportKind::Input)
                .unwrap()
                .wire_bytes,
            64
        );
        assert_eq!(
            reports
                .iter()
                .find(|r| r.kind == ReportKind::Feature)
                .unwrap()
                .wire_bytes,
            8
        );
    }
    #[test]
    fn vendor_report_id_adds_one_byte() {
        let reports = layouts(&[6, 17, 255, 133, 5, 117, 8, 149, 4, 129, 0]).unwrap();
        assert_eq!(reports[0].report_id, Some(5));
        assert_eq!(reports[0].wire_bytes, 5);
    }
    #[test]
    fn malformed_descriptor_never_panics() {
        for raw in [
            vec![0x75],
            vec![0xfe, 9],
            vec![0xb4],
            vec![0xa4],
            vec![0x85, 0],
            vec![0x77, 255, 255, 255, 255, 0x95, 255, 0x81, 0],
        ] {
            assert!(layouts(&raw).is_err());
        }
    }
}
