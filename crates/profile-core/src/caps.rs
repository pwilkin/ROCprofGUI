//! Runtime-overridable import and analysis caps.
//!
//! Every cap keeps its historical value as a safe default and can be adjusted
//! with an environment variable, read once before the process imports anything.
//! This is for operators profiling large live workloads (e.g. a long
//! llama.cpp decode whose kernel-trace CSV or decoded ATT wave-state set
//! exceeds the shipped defaults) where the underlying path streams, pages or is
//! cancellable enough to tolerate a larger bound. The caps stay explicit
//! rejection boundaries — nothing is ever silently sampled; raising a cap only
//! moves the point at which an import is refused.
//!
//! Only caps whose enforcement path is streaming/paged/cancellable and that are
//! not mirrored in the frontend are exposed here. The wave-viewport budgets in
//! [`crate::wave_index`] load each selected wave fully into memory and are
//! mirrored in `src/waveOverview.ts`; they stay fixed until the indexed
//! out-of-core viewer lands.

/// Default main-import record cap (`check_rows`, streamed token limit).
pub const MAX_RECORDS_DEFAULT: usize = 2_000_000;
/// Default per-file byte cap for sidecar / JSON / CSV inputs, in MiB.
pub const MAX_FILE_MIB_DEFAULT: u64 = 256;
/// Default detailed-token cap for hidden-latency analysis.
pub const MAX_HIDDEN_TOKENS_DEFAULT: usize = 4_000_000;
/// Default ATT-manifest trace-file cap.
pub const MAX_ATT_TRACES_DEFAULT: usize = 256;
/// Default ATT-manifest code-object-file cap.
pub const MAX_ATT_CODE_OBJECTS_DEFAULT: usize = 256;

/// A positive integer from `raw`, or `default` when it is absent, blank,
/// unparseable or zero. Zero never disables a cap.
fn parse_usize(raw: Option<String>, default: usize) -> usize {
    raw.and_then(|s| s.trim().parse::<usize>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(default)
}
fn parse_u64(raw: Option<String>, default: u64) -> u64 {
    raw.and_then(|s| s.trim().parse::<u64>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(default)
}

/// Record cap for `check_rows` and the streamed token limit.
/// Override: `ROCPROF_MAX_RECORDS`.
pub fn max_records() -> usize {
    parse_usize(std::env::var("ROCPROF_MAX_RECORDS").ok(), MAX_RECORDS_DEFAULT)
}
/// Per-file byte cap in MiB. Override: `ROCPROF_MAX_FILE_MIB`.
pub fn max_file_mib() -> u64 {
    parse_u64(std::env::var("ROCPROF_MAX_FILE_MIB").ok(), MAX_FILE_MIB_DEFAULT)
}
/// Per-file byte cap in bytes (saturating, so a huge MiB value cannot overflow).
pub fn max_file_bytes() -> u64 {
    max_file_mib().saturating_mul(1024 * 1024)
}
/// Detailed-token cap for hidden-latency analysis.
/// Override: `ROCPROF_MAX_HIDDEN_TOKENS`.
pub fn max_hidden_tokens() -> usize {
    parse_usize(
        std::env::var("ROCPROF_MAX_HIDDEN_TOKENS").ok(),
        MAX_HIDDEN_TOKENS_DEFAULT,
    )
}
/// ATT-manifest trace-file cap. Override: `ROCPROF_MAX_ATT_TRACES`.
pub fn max_att_traces() -> usize {
    parse_usize(std::env::var("ROCPROF_MAX_ATT_TRACES").ok(), MAX_ATT_TRACES_DEFAULT)
}
/// ATT-manifest code-object-file cap. Override: `ROCPROF_MAX_ATT_CODE_OBJECTS`.
pub fn max_att_code_objects() -> usize {
    parse_usize(
        std::env::var("ROCPROF_MAX_ATT_CODE_OBJECTS").ok(),
        MAX_ATT_CODE_OBJECTS_DEFAULT,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_usize_falls_back_on_absent_blank_unparseable_and_zero() {
        assert_eq!(parse_usize(None, 10), 10);
        assert_eq!(parse_usize(Some(String::new()), 10), 10);
        assert_eq!(parse_usize(Some("   ".into()), 10), 10);
        assert_eq!(parse_usize(Some("abc".into()), 10), 10);
        assert_eq!(parse_usize(Some("0".into()), 10), 10);
        assert_eq!(parse_usize(Some("-5".into()), 10), 10);
        assert_eq!(parse_usize(Some("3.5".into()), 10), 10);
    }
    #[test]
    fn parse_usize_accepts_a_trimmed_positive_integer() {
        assert_eq!(parse_usize(Some("  8000000 ".into()), 10), 8_000_000);
        assert_eq!(parse_usize(Some("1".into()), 10), 1);
    }
    #[test]
    fn parse_u64_matches_usize_rules() {
        assert_eq!(parse_u64(None, 256), 256);
        assert_eq!(parse_u64(Some("0".into()), 256), 256);
        assert_eq!(parse_u64(Some("512".into()), 256), 512);
    }
}
