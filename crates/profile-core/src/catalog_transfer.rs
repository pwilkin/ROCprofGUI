use crate::{Capture, Instruction};
use serde::Serialize;
use std::{
    io,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

pub const MAX_PAGE_ROWS: usize = 4096;
pub const MAX_PAGE_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_HEADER_BYTES: usize = 128 * 1024 * 1024;
pub const MAX_INSTRUCTIONS: usize = 2_000_000;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogHeader {
    pub transport: &'static str,
    pub transfer_id: String,
    pub instruction_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observation_count: Option<usize>,
    pub max_page_rows: usize,
    pub capture: Capture,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstructionPage {
    pub transfer_id: String,
    pub capture_id: String,
    pub offset: usize,
    pub total: usize,
    pub instructions: Vec<Instruction>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PageView<'a> {
    transfer_id: &'a str,
    capture_id: &'a str,
    offset: usize,
    total: usize,
    instructions: &'a [Instruction],
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObservationPage {
    pub transfer_id: String,
    pub capture_id: String,
    pub offset: usize,
    pub total: usize,
    pub observations: Vec<Vec<String>>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ObservationPageView<'a> {
    transfer_id: &'a str,
    capture_id: &'a str,
    offset: usize,
    total: usize,
    observations: &'a [Vec<String>],
}

pub struct CatalogTransfer {
    capture: Arc<Capture>,
    transfer_id: String,
    cancelled: Arc<AtomicBool>,
    observation_next: usize,
    observation_last: Option<(usize, usize, usize)>,
    next: usize,
    last: Option<(usize, usize, usize)>,
}

impl CatalogTransfer {
    pub fn new(
        capture: Arc<Capture>,
        transfer_id: String,
        cancelled: Arc<AtomicBool>,
    ) -> Result<Self, String> {
        if transfer_id.is_empty() || transfer_id.len() > 128 || transfer_id.contains('\0') {
            return Err("Invalid catalog transfer identity".into());
        }
        if capture.instructions.len() > MAX_INSTRUCTIONS {
            return Err("Catalog transfer exceeds two million instructions".into());
        }
        if capture
            .pc_sampling_observations
            .as_ref()
            .is_some_and(|o| o.rows.len() > MAX_INSTRUCTIONS)
        {
            return Err("Observation transfer exceeds two million records".into());
        }
        let result = Self {
            capture,
            transfer_id,
            cancelled,
            observation_next: 0,
            observation_last: None,
            next: 0,
            last: None,
        };
        result.check_cancelled()?;
        Ok(result)
    }

    fn check_cancelled(&self) -> Result<(), String> {
        if self.cancelled.load(Ordering::Relaxed) {
            Err("Catalog transfer cancelled".into())
        } else {
            Ok(())
        }
    }

    pub fn header(&self) -> Result<CatalogHeader, String> {
        self.check_cancelled()?;
        let observation_count = self
            .capture
            .pc_sampling_observations
            .as_ref()
            .map(|o| o.rows.len());
        let mut capture = self.capture.clone_without_instructions();
        capture.pc_sampling_observations =
            self.capture.pc_sampling_observations.as_ref().map(|o| {
                Arc::new(crate::model::PcSamplingObservations {
                    columns: o.columns.clone(),
                    rows: Vec::new(),
                })
            });
        let header = CatalogHeader {
            transport: if observation_count.is_some() {
                "instruction-pages-v2"
            } else {
                "instruction-pages-v1"
            },
            observation_count,
            transfer_id: self.transfer_id.clone(),
            instruction_count: self.capture.instructions.len(),
            max_page_rows: MAX_PAGE_ROWS,
            capture,
        };
        if encoded_size(&header, MAX_HEADER_BYTES, &self.cancelled)?.is_none() {
            return Err("Catalog header exceeds 128 MiB".into());
        }
        Ok(header)
    }

    pub fn page(&mut self, offset: usize, rows: usize) -> Result<InstructionPage, String> {
        self.check_cancelled()?;
        if rows == 0 || rows > MAX_PAGE_ROWS {
            return Err("Catalog page must request 1 to 4096 rows".into());
        }
        let total = self.capture.instructions.len();
        if offset > total {
            return Err("Catalog page offset exceeds instruction count".into());
        }
        let retry = self
            .last
            .filter(|(start, _, requested)| *start == offset && *requested == rows);
        if offset != self.next && retry.is_none() {
            return Err(
                "Catalog pages must be requested in order; only the last page may be retried"
                    .into(),
            );
        }
        let end = if let Some((_, end, _)) = retry {
            end
        } else {
            let empty = PageView {
                transfer_id: &self.transfer_id,
                capture_id: &self.capture.id,
                offset,
                total,
                instructions: &[],
            };
            let mut bytes = encoded_size(&empty, MAX_PAGE_BYTES, &self.cancelled)?
                .ok_or("Catalog page identity exceeds byte limit")?;
            let mut end = offset;
            for instruction in self.capture.instructions[offset..].iter().take(rows) {
                let comma = usize::from(end > offset);
                let available = MAX_PAGE_BYTES.saturating_sub(bytes + comma);
                let Some(size) = encoded_size(instruction, available, &self.cancelled)? else {
                    if end == offset {
                        return Err(format!(
                            "Instruction at catalog offset {offset} exceeds the 4 MiB page limit"
                        ));
                    }
                    break;
                };
                bytes += comma + size;
                end += 1;
            }
            end
        };
        let page = InstructionPage {
            transfer_id: self.transfer_id.clone(),
            capture_id: self.capture.id.clone(),
            offset,
            total,
            instructions: self.capture.instructions[offset..end].to_vec(),
        };
        self.check_cancelled()?;
        self.next = end;
        self.last = Some((offset, end, rows));
        Ok(page)
    }

    pub fn observation_page(
        &mut self,
        offset: usize,
        rows: usize,
    ) -> Result<ObservationPage, String> {
        self.check_cancelled()?;
        if rows == 0 || rows > MAX_PAGE_ROWS {
            return Err("Observation page must request 1 to 4096 rows".into());
        }
        let data = &self
            .capture
            .pc_sampling_observations
            .as_ref()
            .ok_or("Capture has no CSV observations")?
            .rows;
        let total = data.len();
        if offset > total {
            return Err("Observation page offset exceeds observation count".into());
        }
        let retry = self
            .observation_last
            .filter(|(start, _, requested)| *start == offset && *requested == rows);
        if offset != self.observation_next && retry.is_none() {
            return Err(
                "Observation pages must be requested in order; only the last page may be retried"
                    .into(),
            );
        }
        let end = if let Some((_, end, _)) = retry {
            end
        } else {
            let empty = ObservationPageView {
                transfer_id: &self.transfer_id,
                capture_id: &self.capture.id,
                offset,
                total,
                observations: &[],
            };
            let mut bytes = encoded_size(&empty, MAX_PAGE_BYTES, &self.cancelled)?
                .ok_or("Observation page identity exceeds byte limit")?;
            let mut end = offset;
            for observation in data[offset..].iter().take(rows) {
                let comma = usize::from(end > offset);
                let available = MAX_PAGE_BYTES.saturating_sub(bytes + comma);
                let Some(size) = encoded_size(observation, available, &self.cancelled)? else {
                    if end == offset {
                        return Err(format!(
                            "CSV observation at offset {offset} exceeds the 4 MiB page limit"
                        ));
                    }
                    break;
                };
                bytes += comma + size;
                end += 1;
            }
            end
        };
        let page = ObservationPage {
            transfer_id: self.transfer_id.clone(),
            capture_id: self.capture.id.clone(),
            offset,
            total,
            observations: data[offset..end].to_vec(),
        };
        self.check_cancelled()?;
        self.observation_next = end;
        self.observation_last = Some((offset, end, rows));
        Ok(page)
    }

    pub fn commit(self) -> Result<Arc<Capture>, String> {
        self.check_cancelled()?;
        if self.next != self.capture.instructions.len()
            || self.observation_next
                != self
                    .capture
                    .pc_sampling_observations
                    .as_ref()
                    .map_or(0, |o| o.rows.len())
        {
            return Err("Catalog transfer is incomplete".into());
        }
        Ok(self.capture)
    }
}

struct ByteCounter<'a> {
    size: usize,
    limit: usize,
    exceeded: bool,
    cancelled: &'a AtomicBool,
}
impl io::Write for ByteCounter<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.cancelled.load(Ordering::Relaxed) {
            return Err(io::Error::other("Catalog transfer cancelled"));
        }
        if bytes.len() > self.limit.saturating_sub(self.size) {
            self.exceeded = true;
            return Err(io::Error::other("Catalog transfer byte limit"));
        }
        self.size += bytes.len();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
pub(crate) fn encoded_size(
    value: &impl Serialize,
    limit: usize,
    cancelled: &AtomicBool,
) -> Result<Option<usize>, String> {
    let mut counter = ByteCounter {
        size: 0,
        limit,
        exceeded: false,
        cancelled,
    };
    let result = serde_json::to_writer(&mut counter, value);
    if cancelled.load(Ordering::Relaxed) {
        return Err("Catalog transfer cancelled".into());
    }
    if counter.exceeded {
        return Ok(None);
    }
    result.map_err(|error| error.to_string())?;
    Ok(Some(counter.size))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn encoded_budget_counts_json_escaping_and_observes_late_cancellation() {
        let flag = AtomicBool::new(true);
        assert!(encoded_size(&"cancelled", 100, &flag).is_err());
        flag.store(false, Ordering::Relaxed);
        let value = "\n\"µ";
        let exact = serde_json::to_vec(value).unwrap().len();
        assert_eq!(encoded_size(&value, exact, &flag).unwrap(), Some(exact));
        assert_eq!(encoded_size(&value, exact - 1, &flag).unwrap(), None);
        struct CancelWhileSerializing<'a>(&'a AtomicBool);
        impl Serialize for CancelWhileSerializing<'_> {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                let result = serializer.serialize_u32(1);
                self.0.store(true, Ordering::Relaxed);
                result
            }
        }
        assert!(encoded_size(&CancelWhileSerializing(&flag), 10, &flag).is_err());
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogBatchHeader {
    pub transport: &'static str,
    pub transfer_id: String,
    pub captures: Vec<CatalogHeader>,
}

pub struct CatalogBatch {
    transfer_id: String,
    transfers: Vec<CatalogTransfer>,
    cancelled: Arc<AtomicBool>,
}
impl CatalogBatch {
    pub fn new(
        captures: Vec<Arc<Capture>>,
        transfer_id: String,
        cancelled: Arc<AtomicBool>,
    ) -> Result<Self, String> {
        if captures.is_empty()
            || captures.len() > 7
            || captures.iter().filter(|c| c.id != "demo").count() > 6
        {
            return Err("A catalog batch supports six native captures and one demo".into());
        }
        if transfer_id.is_empty() || transfer_id.len() > 96 || transfer_id.contains('\0') {
            return Err("Invalid catalog batch identity".into());
        }
        let mut ids = std::collections::BTreeSet::new();
        for capture in &captures {
            if capture.id.is_empty() || !ids.insert(capture.id.clone()) {
                return Err("Catalog batch capture identities must be nonempty and unique".into());
            }
        }
        let transfers = captures
            .into_iter()
            .enumerate()
            .map(|(index, capture)| {
                CatalogTransfer::new(capture, format!("{transfer_id}:{index}"), cancelled.clone())
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            transfer_id,
            transfers,
            cancelled,
        })
    }

    pub fn header(&self) -> Result<CatalogBatchHeader, String> {
        let header = CatalogBatchHeader {
            transport: "catalog-batch-v1",
            transfer_id: self.transfer_id.clone(),
            captures: self
                .transfers
                .iter()
                .map(CatalogTransfer::header)
                .collect::<Result<_, _>>()?,
        };
        if encoded_size(&header, crate::project::MAX_STAGED_BYTES, &self.cancelled)?.is_none() {
            return Err("Catalog batch header exceeds project staging byte limit".into());
        }
        Ok(header)
    }

    pub fn page(
        &mut self,
        capture_index: usize,
        offset: usize,
        rows: usize,
    ) -> Result<InstructionPage, String> {
        self.transfers
            .get_mut(capture_index)
            .ok_or("Catalog batch capture index is out of range")?
            .page(offset, rows)
    }

    pub fn observation_page(
        &mut self,
        capture_index: usize,
        offset: usize,
        rows: usize,
    ) -> Result<ObservationPage, String> {
        self.transfers
            .get_mut(capture_index)
            .ok_or("Catalog batch capture index is out of range")?
            .observation_page(offset, rows)
    }

    pub fn commit(self) -> Result<Vec<Arc<Capture>>, String> {
        let captures = self
            .transfers
            .into_iter()
            .map(CatalogTransfer::commit)
            .collect::<Result<Vec<_>, _>>()?;
        if self.cancelled.load(Ordering::Relaxed) {
            return Err("Catalog batch cancelled".into());
        }
        Ok(captures)
    }
}
