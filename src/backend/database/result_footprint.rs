use std::fmt;

use momento_common::llm::result_stream::{MAX_LLM_RESULT_BYTES, MAX_LLM_RESULT_INPUTS};
use momento_common::llm::MAX_LLM_RESULT_RECORDS;

pub const MAX_DURABLE_ERROR_BYTES: u64 = 4 * 1024;
pub const MAX_LLM_RESULT_CLEANUP_ROWS: u64 = 256;
pub const MAX_LLM_RESULT_PERSIST_BATCH_BYTES: u64 = 4 * 1024 * 1024;

const SQLITE_RECORD_HEADER_BYTES: u64 = 16;
const SQLITE_BTREE_CELL_BYTES: u64 = 16;
const STAGING_FIXED_ROW_BYTES: u64 = 96;
const STAGING_INDEX_ENTRY_BYTES: u64 = 96;
const RESULT_FIXED_ROWS_AND_INDEXES_BYTES: u64 = 64 * 1024;
const RECEIVE_AND_FINALIZE_WAL_MULTIPLIER: u64 = 2;
const PAGE_SPLIT_MULTIPLIER: u64 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TaskFootprintDescriptor {
    task: &'static str,
    final_row_bytes: u64,
    final_rows_per_primary_record: u64,
    final_index_entries_per_primary_record: u64,
    artifact_plan_bytes_per_primary_record: u64,
}

const TASK_FOOTPRINTS: [TaskFootprintDescriptor; 7] = [
    task("ocr", 256, 1, 2, 0),
    task("image_tagging", 256, 1, 2, 0),
    task("image_clustering", 4 * 1024, 2, 4, 0),
    task("image_aesthetics", 256, 2, 3, 0),
    task("face_detection", 3 * 1024, 2, 4, 2 * 1024),
    task("screenshot_detection", 256, 2, 3, 0),
    task("document_detection", 256, 2, 3, 0),
];

const fn task(
    task: &'static str,
    final_row_bytes: u64,
    final_rows_per_primary_record: u64,
    final_index_entries_per_primary_record: u64,
    artifact_plan_bytes_per_primary_record: u64,
) -> TaskFootprintDescriptor {
    TaskFootprintDescriptor {
        task,
        final_row_bytes,
        final_rows_per_primary_record,
        final_index_entries_per_primary_record,
        artifact_plan_bytes_per_primary_record,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResultSqliteFootprint {
    pub construction_max_growth_bytes: u64,
    pub cleanup_recovery_max_growth_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SqliteFootprintRegistry {
    page_size_bytes: u64,
    pub result_rejection_max_growth_bytes: u64,
    pub result_cleanup_recovery_max_growth_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultFootprintError {
    InvalidPageSize,
    InvalidManifest,
    UnknownTask,
    ArithmeticOverflow,
}

impl fmt::Display for ResultFootprintError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidPageSize => "SQLite result footprint page size is invalid",
            Self::InvalidManifest => "LLM result manifest is outside its footprint contract",
            Self::UnknownTask => "LLM result task has no SQLite footprint descriptor",
            Self::ArithmeticOverflow => "LLM result SQLite footprint overflowed",
        })
    }
}

impl std::error::Error for ResultFootprintError {}

impl SqliteFootprintRegistry {
    pub fn new(page_size_bytes: u64) -> Result<Self, ResultFootprintError> {
        if !(512..=65_536).contains(&page_size_bytes) || !page_size_bytes.is_power_of_two() {
            return Err(ResultFootprintError::InvalidPageSize);
        }
        let rejection_payload = MAX_DURABLE_ERROR_BYTES
            .checked_add(8 * SQLITE_BTREE_CELL_BYTES)
            .ok_or(ResultFootprintError::ArithmeticOverflow)?;
        let cleanup_payload = MAX_LLM_RESULT_PERSIST_BATCH_BYTES
            .checked_add(
                MAX_LLM_RESULT_CLEANUP_ROWS
                    .checked_mul(STAGING_FIXED_ROW_BYTES + 2 * STAGING_INDEX_ENTRY_BYTES)
                    .ok_or(ResultFootprintError::ArithmeticOverflow)?,
            )
            .and_then(|value| value.checked_mul(RECEIVE_AND_FINALIZE_WAL_MULTIPLIER))
            .ok_or(ResultFootprintError::ArithmeticOverflow)?;
        Ok(Self {
            page_size_bytes,
            result_rejection_max_growth_bytes: align_pages(rejection_payload, page_size_bytes)?,
            result_cleanup_recovery_max_growth_bytes: align_pages(
                cleanup_payload,
                page_size_bytes,
            )?,
        })
    }

    pub fn result(
        &self,
        task: &str,
        record_count: u32,
        byte_size: u64,
    ) -> Result<ResultSqliteFootprint, ResultFootprintError> {
        if record_count == 0
            || record_count > MAX_LLM_RESULT_RECORDS
            || !(momento_common::llm::RESULT_RECORD_HEADER_BYTES as u64..=MAX_LLM_RESULT_BYTES)
                .contains(&byte_size)
        {
            return Err(ResultFootprintError::InvalidManifest);
        }
        let descriptor = TASK_FOOTPRINTS
            .iter()
            .find(|descriptor| descriptor.task == task)
            .ok_or(ResultFootprintError::UnknownTask)?;
        let record_count = u64::from(record_count);
        let staging_rows = record_count
            .checked_mul(
                STAGING_FIXED_ROW_BYTES
                    + SQLITE_RECORD_HEADER_BYTES
                    + 2 * STAGING_INDEX_ENTRY_BYTES,
            )
            .ok_or(ResultFootprintError::ArithmeticOverflow)?;
        let staging_pages = record_count
            .checked_add(MAX_LLM_RESULT_CLEANUP_ROWS - 1)
            .map(|value| value / MAX_LLM_RESULT_CLEANUP_ROWS)
            .ok_or(ResultFootprintError::ArithmeticOverflow)?;
        let staging_page_alignment = staging_pages
            .checked_mul(self.page_size_bytes)
            .ok_or(ResultFootprintError::ArithmeticOverflow)?;
        let final_rows = record_count
            .checked_mul(descriptor.final_rows_per_primary_record)
            .and_then(|rows| {
                rows.checked_mul(descriptor.final_row_bytes + SQLITE_RECORD_HEADER_BYTES)
            })
            .ok_or(ResultFootprintError::ArithmeticOverflow)?;
        let final_indexes = record_count
            .checked_mul(descriptor.final_index_entries_per_primary_record)
            .and_then(|entries| entries.checked_mul(STAGING_INDEX_ENTRY_BYTES))
            .ok_or(ResultFootprintError::ArithmeticOverflow)?;
        let artifact_plan = record_count
            .checked_mul(descriptor.artifact_plan_bytes_per_primary_record)
            .ok_or(ResultFootprintError::ArithmeticOverflow)?;
        let input_correlation = (MAX_LLM_RESULT_INPUTS as u64)
            .checked_mul(128)
            .ok_or(ResultFootprintError::ArithmeticOverflow)?;
        let construction_payload = [
            RESULT_FIXED_ROWS_AND_INDEXES_BYTES,
            input_correlation,
            byte_size,
            staging_rows,
            staging_page_alignment,
            final_rows,
            final_indexes,
            artifact_plan,
            self.result_cleanup_recovery_max_growth_bytes,
        ]
        .into_iter()
        .try_fold(0_u64, |total, value| {
            total
                .checked_add(value)
                .ok_or(ResultFootprintError::ArithmeticOverflow)
        })?
        .checked_mul(PAGE_SPLIT_MULTIPLIER)
        .and_then(|value| value.checked_mul(RECEIVE_AND_FINALIZE_WAL_MULTIPLIER))
        .ok_or(ResultFootprintError::ArithmeticOverflow)?;
        Ok(ResultSqliteFootprint {
            construction_max_growth_bytes: align_pages(construction_payload, self.page_size_bytes)?,
            cleanup_recovery_max_growth_bytes: self.result_cleanup_recovery_max_growth_bytes,
        })
    }

    pub fn staging_page(
        &self,
        record_count: usize,
        normalized_payload_bytes: u64,
    ) -> Result<u64, ResultFootprintError> {
        if record_count == 0
            || record_count > MAX_LLM_RESULT_CLEANUP_ROWS as usize
            || normalized_payload_bytes > MAX_LLM_RESULT_PERSIST_BATCH_BYTES
        {
            return Err(ResultFootprintError::InvalidManifest);
        }
        let record_count =
            u64::try_from(record_count).map_err(|_| ResultFootprintError::ArithmeticOverflow)?;
        let row_bytes = record_count
            .checked_mul(
                STAGING_FIXED_ROW_BYTES
                    + SQLITE_RECORD_HEADER_BYTES
                    + 2 * STAGING_INDEX_ENTRY_BYTES,
            )
            .and_then(|value| value.checked_add(normalized_payload_bytes))
            .and_then(|value| value.checked_mul(PAGE_SPLIT_MULTIPLIER))
            .and_then(|value| value.checked_mul(RECEIVE_AND_FINALIZE_WAL_MULTIPLIER))
            .ok_or(ResultFootprintError::ArithmeticOverflow)?;
        align_pages(row_bytes, self.page_size_bytes)
    }

    pub fn persistence(&self, task: &str, record_count: u32) -> Result<u64, ResultFootprintError> {
        if record_count == 0 || record_count > MAX_LLM_RESULT_RECORDS {
            return Err(ResultFootprintError::InvalidManifest);
        }
        let descriptor = TASK_FOOTPRINTS
            .iter()
            .find(|descriptor| descriptor.task == task)
            .ok_or(ResultFootprintError::UnknownTask)?;
        let record_count = u64::from(record_count);
        let final_rows = record_count
            .checked_mul(descriptor.final_rows_per_primary_record)
            .and_then(|rows| {
                rows.checked_mul(descriptor.final_row_bytes + SQLITE_RECORD_HEADER_BYTES)
            })
            .ok_or(ResultFootprintError::ArithmeticOverflow)?;
        let final_indexes = record_count
            .checked_mul(descriptor.final_index_entries_per_primary_record)
            .and_then(|entries| entries.checked_mul(STAGING_INDEX_ENTRY_BYTES))
            .ok_or(ResultFootprintError::ArithmeticOverflow)?;
        let artifact_plan = record_count
            .checked_mul(descriptor.artifact_plan_bytes_per_primary_record)
            .ok_or(ResultFootprintError::ArithmeticOverflow)?;
        let payload = RESULT_FIXED_ROWS_AND_INDEXES_BYTES
            .checked_add(final_rows)
            .and_then(|value| value.checked_add(final_indexes))
            .and_then(|value| value.checked_add(artifact_plan))
            .and_then(|value| value.checked_mul(PAGE_SPLIT_MULTIPLIER))
            .and_then(|value| value.checked_mul(RECEIVE_AND_FINALIZE_WAL_MULTIPLIER))
            .ok_or(ResultFootprintError::ArithmeticOverflow)?;
        align_pages(payload, self.page_size_bytes)
    }

    pub fn supported_tasks(&self) -> impl ExactSizeIterator<Item = &'static str> {
        TASK_FOOTPRINTS.iter().map(|descriptor| descriptor.task)
    }
}

fn align_pages(value: u64, page_size_bytes: u64) -> Result<u64, ResultFootprintError> {
    value
        .checked_add(page_size_bytes - 1)
        .map(|value| value & !(page_size_bytes - 1))
        .ok_or(ResultFootprintError::ArithmeticOverflow)
}
