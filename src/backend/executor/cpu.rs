use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Arc, OnceLock};
use std::thread::JoinHandle;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use crossbeam_channel::Receiver;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::{oneshot, Notify};

use super::{
    ControlRequestKind, ControlResponse, ExecutorDomain, ExecutorError, ExecutorErrorKind,
    OperationSpec, ParsedControlRequest, MAX_PROBE_OUTPUT_BYTES,
};
use crate::executor::process::{ChildProcessCompletion, ChildProcessSpec};
use crate::processor::metadata::reverse_geocoding::ReverseGeocoderSnapshot;
use crate::runtime::scheduler::{SchedulerIngress, SubmissionMode};
use crate::runtime::ConfigFileIdentity;

pub use super::bounded_json::{
    ParsedExifMetadata, ParsedFfprobeMetadata, ParsedSupplementalMetadata,
};

const MAX_HASH_INPUT_BYTES: usize = 1024 * 1024;
const MAX_AUTH_SOURCE_BYTES: usize = 256;
const MAX_AUTH_IDENTITY_BYTES: usize = 1024;
const MAX_PASSWORD_HASH_BYTES: usize = 512;
const MAX_JSON_INPUT_BYTES: usize = 1024 * 1024;
const MAX_JSON_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
const MAX_METADATA_JSON_INPUT_BYTES: usize = 4 * 1024 * 1024;
pub const REVERSE_GEOCODER_MAX_RUNTIME_BYTES: usize = 48 * 1024 * 1024;
const REVERSE_GEOCODER_MAX_BUILD_TEMP_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug)]
pub struct DerivedMediaLocation {
    pub geohash: Option<String>,
    pub city: Option<String>,
    pub state: Option<String>,
    pub country: Option<String>,
}

#[derive(Debug)]
pub struct CronCatchUpPage {
    pub latest_due: Option<DateTime<Utc>>,
    pub next: DateTime<Utc>,
    pub continuation_required: bool,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlaceIdentityDto {
    pub city: String,
    pub state: Option<String>,
    pub country: String,
}

pub struct Sha256Session {
    hasher: Sha256,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum MetadataJsonKind {
    Supplemental,
    ExifTool,
    Ffprobe,
}

pub(crate) enum CpuOperation {
    Probe {
        sequence: u64,
    },
    Sha256 {
        bytes: Vec<u8>,
    },
    StartSha256Session,
    UpdateSha256Session {
        session: Sha256Session,
        bytes: Vec<u8>,
    },
    FinishSha256Session {
        session: Sha256Session,
    },
    ValidateJson {
        bytes: Vec<u8>,
    },
    ParseControlRequest {
        kind: ControlRequestKind,
        bytes: Vec<u8>,
    },
    SerializeControlResponse {
        response: Box<ControlResponse>,
    },
    SerializeBackupMetadata {
        metadata: serde_json::Value,
    },
    DecodePlaceIdentity {
        place_id: String,
    },
    BuildPlaceSummaries {
        rows: Vec<crate::database::operations::PlaceRecord>,
    },
    ParseMetadataJson {
        bytes: Vec<u8>,
        kind: MetadataJsonKind,
    },
    RenderRequestLogPayload {
        bytes: Vec<u8>,
        maximum_bytes: usize,
        truncated: bool,
    },
    AuthAttemptDigests {
        source: Vec<u8>,
        identity: Vec<u8>,
    },
    HashPassword {
        password: String,
    },
    VerifyPassword {
        password: String,
        hash: Option<String>,
        dummy_hash: String,
    },
    PrepareLlmCronUpdate {
        identity: ConfigFileIdentity,
        contents: String,
        config_field_name: &'static str,
        feature_name: &'static str,
        cron_expression: String,
    },
    PrepareAdminPasswordResetUpdate {
        identity: ConfigFileIdentity,
        contents: String,
    },
    InitializeReverseGeocoder,
    ComputeCronCatchUpPage {
        cron_expression: String,
        after: DateTime<Utc>,
        now: DateTime<Utc>,
        timezone: Tz,
        maximum_occurrences: u16,
    },
    DeriveMediaLocation {
        geocoder: Option<ReverseGeocoderSnapshot>,
        latitude: Option<f64>,
        longitude: Option<f64>,
    },
    CompareDeduplicatePage {
        page: crate::processor::deduplicator::DeduplicateComparisonPage,
    },
    MeasureDeduplicateGroupPage {
        page: crate::processor::deduplicator::DeduplicateGroupPage,
    },
    CompareFaceGroupPage {
        page: crate::processor::face_detection::FaceComparisonPage,
    },
    ReduceFaceRepresentativePage {
        page: crate::processor::face_detection::FaceRepresentativeReductionPage,
    },
    SuperviseChildProcess {
        spec: ChildProcessSpec,
    },
}

impl CpuOperation {
    fn name(&self) -> &'static str {
        match self {
            Self::Probe { .. } => "cpu_probe",
            Self::Sha256 { .. } => "sha256",
            Self::StartSha256Session => "start_sha256_session",
            Self::UpdateSha256Session { .. } => "update_sha256_session",
            Self::FinishSha256Session { .. } => "finish_sha256_session",
            Self::ValidateJson { .. } => "validate_json",
            Self::ParseControlRequest { .. } => "parse_control_request",
            Self::SerializeControlResponse { .. } => "serialize_control_response",
            Self::SerializeBackupMetadata { .. } => "serialize_backup_metadata",
            Self::DecodePlaceIdentity { .. } => "decode_place_identity",
            Self::BuildPlaceSummaries { .. } => "build_place_summaries",
            Self::ParseMetadataJson { .. } => "parse_metadata_json",
            Self::RenderRequestLogPayload { .. } => "render_request_log_payload",
            Self::AuthAttemptDigests { .. } => "auth_attempt_digests",
            Self::HashPassword { .. } => "hash_password",
            Self::VerifyPassword { .. } => "verify_password",
            Self::PrepareLlmCronUpdate { .. } => "prepare_llm_cron_update",
            Self::PrepareAdminPasswordResetUpdate { .. } => "prepare_admin_password_reset_update",
            Self::InitializeReverseGeocoder => "initialize_reverse_geocoder",
            Self::ComputeCronCatchUpPage { .. } => "compute_cron_catch_up_page",
            Self::DeriveMediaLocation { .. } => "derive_media_location",
            Self::CompareDeduplicatePage { .. } => "compare_deduplicate_page",
            Self::MeasureDeduplicateGroupPage { .. } => "measure_deduplicate_group_page",
            Self::CompareFaceGroupPage { .. } => "compare_face_group_page",
            Self::ReduceFaceRepresentativePage { .. } => "reduce_face_representative_page",
            Self::SuperviseChildProcess { .. } => "supervise_child_process",
        }
    }

    pub(crate) fn spec(&self) -> OperationSpec {
        match self {
            Self::Probe { .. } => OperationSpec {
                domain: ExecutorDomain::Cpu,
                maximum_input_bytes: size_of::<u64>(),
                maximum_output_bytes: MAX_PROBE_OUTPUT_BYTES,
                maximum_temporary_bytes: 0,
            },
            Self::Sha256 { .. } => OperationSpec {
                domain: ExecutorDomain::Cpu,
                maximum_input_bytes: MAX_HASH_INPUT_BYTES,
                maximum_output_bytes: 32,
                maximum_temporary_bytes: 256,
            },
            Self::StartSha256Session => OperationSpec {
                domain: ExecutorDomain::Cpu,
                maximum_input_bytes: 0,
                maximum_output_bytes: 256,
                maximum_temporary_bytes: 0,
            },
            Self::UpdateSha256Session { .. } => OperationSpec {
                domain: ExecutorDomain::Cpu,
                maximum_input_bytes: MAX_HASH_INPUT_BYTES + 256,
                maximum_output_bytes: 256,
                maximum_temporary_bytes: 256,
            },
            Self::FinishSha256Session { .. } => OperationSpec {
                domain: ExecutorDomain::Cpu,
                maximum_input_bytes: 256,
                maximum_output_bytes: 64,
                maximum_temporary_bytes: 256,
            },
            Self::ValidateJson { .. } => OperationSpec {
                domain: ExecutorDomain::Cpu,
                maximum_input_bytes: MAX_JSON_INPUT_BYTES,
                maximum_output_bytes: MAX_JSON_INPUT_BYTES,
                maximum_temporary_bytes: MAX_JSON_INPUT_BYTES,
            },
            Self::ParseControlRequest { .. } => OperationSpec {
                domain: ExecutorDomain::Cpu,
                maximum_input_bytes: MAX_JSON_INPUT_BYTES,
                maximum_output_bytes: 2 * MAX_JSON_INPUT_BYTES,
                maximum_temporary_bytes: 3 * MAX_JSON_INPUT_BYTES,
            },
            Self::SerializeControlResponse { .. } => OperationSpec {
                domain: ExecutorDomain::Cpu,
                maximum_input_bytes: 4 * MAX_JSON_INPUT_BYTES,
                maximum_output_bytes: 4 * MAX_JSON_INPUT_BYTES,
                maximum_temporary_bytes: 4 * MAX_JSON_INPUT_BYTES,
            },
            Self::SerializeBackupMetadata { .. } => OperationSpec {
                domain: ExecutorDomain::Cpu,
                maximum_input_bytes: MAX_JSON_INPUT_BYTES,
                maximum_output_bytes: MAX_JSON_INPUT_BYTES,
                maximum_temporary_bytes: MAX_JSON_INPUT_BYTES,
            },
            Self::DecodePlaceIdentity { .. } => OperationSpec {
                domain: ExecutorDomain::Cpu,
                maximum_input_bytes: 4 * 1024,
                maximum_output_bytes: 4 * 1024,
                maximum_temporary_bytes: 8 * 1024,
            },
            Self::BuildPlaceSummaries { .. } => OperationSpec {
                domain: ExecutorDomain::Cpu,
                maximum_input_bytes: MAX_JSON_INPUT_BYTES,
                maximum_output_bytes: MAX_JSON_INPUT_BYTES,
                maximum_temporary_bytes: MAX_JSON_INPUT_BYTES,
            },
            Self::ParseMetadataJson { .. } => OperationSpec {
                domain: ExecutorDomain::Cpu,
                maximum_input_bytes: MAX_METADATA_JSON_INPUT_BYTES,
                maximum_output_bytes: super::bounded_json::MAXIMUM_NORMALIZED_JSON_BYTES,
                maximum_temporary_bytes: super::bounded_json::MAXIMUM_NORMALIZED_JSON_BYTES,
            },
            Self::RenderRequestLogPayload { .. } => OperationSpec {
                domain: ExecutorDomain::Cpu,
                maximum_input_bytes: MAX_JSON_INPUT_BYTES + 2 * size_of::<usize>(),
                maximum_output_bytes: MAX_JSON_INPUT_BYTES,
                maximum_temporary_bytes: 2 * MAX_JSON_INPUT_BYTES,
            },
            Self::AuthAttemptDigests { .. } => OperationSpec {
                domain: ExecutorDomain::Cpu,
                maximum_input_bytes: MAX_AUTH_SOURCE_BYTES + MAX_AUTH_IDENTITY_BYTES,
                maximum_output_bytes: 64,
                maximum_temporary_bytes: 256,
            },
            Self::HashPassword { .. } => OperationSpec {
                domain: ExecutorDomain::Cpu,
                maximum_input_bytes: crate::auth::MAX_AUTH_PASSWORD_BYTES,
                maximum_output_bytes: MAX_PASSWORD_HASH_BYTES,
                maximum_temporary_bytes: crate::runtime::ARGON2_WORKSPACE_BYTES as usize,
            },
            Self::VerifyPassword { .. } => OperationSpec {
                domain: ExecutorDomain::Cpu,
                maximum_input_bytes: crate::auth::MAX_AUTH_PASSWORD_BYTES
                    + 2 * MAX_PASSWORD_HASH_BYTES,
                maximum_output_bytes: size_of::<bool>(),
                maximum_temporary_bytes: crate::runtime::ARGON2_WORKSPACE_BYTES as usize,
            },
            Self::PrepareLlmCronUpdate { .. } => OperationSpec {
                domain: ExecutorDomain::Cpu,
                maximum_input_bytes: crate::runtime::config_bootstrap::MAX_CONFIG_BYTES as usize
                    + crate::runtime::config_bootstrap::MAX_CONFIG_PATH_BYTES
                    + 1024,
                maximum_output_bytes: crate::runtime::config_bootstrap::MAX_CONFIG_BYTES as usize
                    + 256 * 1024,
                maximum_temporary_bytes: 2 * crate::runtime::config_bootstrap::MAX_CONFIG_BYTES
                    as usize,
            },
            Self::PrepareAdminPasswordResetUpdate { .. } => OperationSpec {
                domain: ExecutorDomain::Cpu,
                maximum_input_bytes: crate::runtime::config_bootstrap::MAX_CONFIG_BYTES as usize
                    + crate::runtime::config_bootstrap::MAX_CONFIG_PATH_BYTES,
                maximum_output_bytes: crate::runtime::config_bootstrap::MAX_CONFIG_BYTES as usize
                    + 256 * 1024,
                maximum_temporary_bytes: 2 * crate::runtime::config_bootstrap::MAX_CONFIG_BYTES
                    as usize,
            },
            Self::InitializeReverseGeocoder => OperationSpec {
                domain: ExecutorDomain::Cpu,
                maximum_input_bytes: 0,
                maximum_output_bytes: REVERSE_GEOCODER_MAX_RUNTIME_BYTES,
                maximum_temporary_bytes: REVERSE_GEOCODER_MAX_BUILD_TEMP_BYTES,
            },
            Self::ComputeCronCatchUpPage { .. } => OperationSpec {
                domain: ExecutorDomain::Cpu,
                maximum_input_bytes: 512,
                maximum_output_bytes: 128,
                maximum_temporary_bytes: 64 * 1024,
            },
            Self::DeriveMediaLocation { .. } => OperationSpec {
                domain: ExecutorDomain::Cpu,
                maximum_input_bytes: 2 * size_of::<Option<f64>>(),
                maximum_output_bytes: 4 * 1024,
                maximum_temporary_bytes: 4 * 1024,
            },
            Self::CompareDeduplicatePage { .. }
            | Self::MeasureDeduplicateGroupPage { .. }
            | Self::CompareFaceGroupPage { .. }
            | Self::ReduceFaceRepresentativePage { .. } => OperationSpec {
                domain: ExecutorDomain::Cpu,
                maximum_input_bytes: 2 * 1024 * 1024,
                maximum_output_bytes: 64 * 1024,
                maximum_temporary_bytes: 64 * 1024,
            },
            Self::SuperviseChildProcess { spec } => OperationSpec {
                domain: ExecutorDomain::Cpu,
                maximum_input_bytes: spec.maximum_input_bytes(),
                maximum_output_bytes: spec.maximum_output_bytes(),
                maximum_temporary_bytes: 2 * 16 * 1024,
            },
        }
    }
}

pub(crate) enum CpuOutput {
    Probe {
        sequence: u64,
        thread_name: String,
    },
    Sha256([u8; 32]),
    Sha256SessionStarted(Sha256Session),
    Sha256SessionUpdated {
        session: Sha256Session,
        bytes: Vec<u8>,
    },
    Sha256SessionFinished(String),
    JsonValidated(Vec<u8>),
    ControlRequestParsed(ParsedControlRequest),
    ControlResponseSerialized(Vec<u8>),
    BackupMetadataSerialized(String),
    PlaceIdentityDecoded(PlaceIdentityDto),
    PlaceSummariesBuilt(Vec<crate::models::PlaceSummary>),
    SupplementalMetadataParsed(ParsedSupplementalMetadata),
    ExifMetadataParsed(ParsedExifMetadata),
    FfprobeMetadataParsed(ParsedFfprobeMetadata),
    RequestLogPayload(String),
    AuthAttemptDigests {
        source: [u8; 32],
        identity: [u8; 32],
    },
    PasswordHash(String),
    PasswordVerified(bool),
    LlmCronUpdate {
        contents: String,
        config: Box<crate::config::Config>,
    },
    AdminPasswordResetUpdate {
        contents: String,
        config: Box<crate::config::Config>,
    },
    ReverseGeocoderInitialized(ReverseGeocoderSnapshot),
    CronCatchUpPage(CronCatchUpPage),
    MediaLocation(DerivedMediaLocation),
    DeduplicateCpuResult(crate::processor::deduplicator::DeduplicateCpuResult),
    FaceGroupCpuResult(crate::processor::face_detection::FaceGroupCpuResult),
    ChildProcessSupervised(ChildProcessCompletion),
}

impl CpuOutput {
    fn mismatch(self, operation: &'static str) -> ExecutorError {
        let actual = match self {
            Self::Probe { .. } => "probe",
            Self::Sha256(_) => "sha256",
            Self::Sha256SessionStarted(_) => "sha256_session_started",
            Self::Sha256SessionUpdated { .. } => "sha256_session_updated",
            Self::Sha256SessionFinished(_) => "sha256_session_finished",
            Self::JsonValidated(_) => "json_validated",
            Self::ControlRequestParsed(_) => "control_request_parsed",
            Self::ControlResponseSerialized(_) => "control_response_serialized",
            Self::BackupMetadataSerialized(_) => "backup_metadata_serialized",
            Self::PlaceIdentityDecoded(_) => "place_identity_decoded",
            Self::PlaceSummariesBuilt(_) => "place_summaries_built",
            Self::SupplementalMetadataParsed(_) => "supplemental_metadata_parsed",
            Self::ExifMetadataParsed(_) => "exif_metadata_parsed",
            Self::FfprobeMetadataParsed(_) => "ffprobe_metadata_parsed",
            Self::RequestLogPayload(_) => "request_log_payload",
            Self::AuthAttemptDigests { .. } => "auth_attempt_digests",
            Self::PasswordHash(_) => "password_hash",
            Self::PasswordVerified(_) => "password_verified",
            Self::LlmCronUpdate { .. } => "llm_cron_update",
            Self::AdminPasswordResetUpdate { .. } => "admin_password_reset_update",
            Self::ReverseGeocoderInitialized(_) => "reverse_geocoder_initialized",
            Self::CronCatchUpPage(_) => "cron_catch_up_page",
            Self::MediaLocation(_) => "media_location",
            Self::DeduplicateCpuResult(_) => "deduplicate_cpu_result",
            Self::FaceGroupCpuResult(_) => "face_group_cpu_result",
            Self::ChildProcessSupervised(_) => "child_process_supervised",
        };
        ExecutorError::new(
            ExecutorErrorKind::Internal,
            operation,
            format!("CPU executor returned mismatched output {actual}"),
        )
    }
}

pub(crate) struct CpuCommand {
    operation: CpuOperation,
    reply: oneshot::Sender<Result<CpuOutput, ExecutorError>>,
}

impl CpuCommand {
    pub(crate) fn new(
        operation: CpuOperation,
        reply: oneshot::Sender<Result<CpuOutput, ExecutorError>>,
    ) -> Self {
        Self { operation, reply }
    }

    pub(crate) fn reject(self, error: ExecutorError) {
        let _ = self.reply.send(Err(error));
    }
}

#[derive(Clone)]
pub struct CpuExecutorHandle {
    ingress: SchedulerIngress,
    reverse_geocoder: Arc<OnceLock<ReverseGeocoderSnapshot>>,
}

impl CpuExecutorHandle {
    pub(crate) fn new(ingress: SchedulerIngress) -> Self {
        Self {
            ingress,
            reverse_geocoder: Arc::new(OnceLock::new()),
        }
    }

    pub async fn probe_durable(&self, sequence: u64) -> Result<(u64, String), ExecutorError> {
        let output = self
            .submit(CpuOperation::Probe { sequence }, SubmissionMode::Durable)
            .await?;
        match output {
            CpuOutput::Probe {
                sequence,
                thread_name,
            } => Ok((sequence, thread_name)),
            output => Err(output.mismatch("cpu_probe")),
        }
    }

    pub async fn sha256_durable(&self, bytes: Vec<u8>) -> Result<[u8; 32], ExecutorError> {
        validate_hash_input(&bytes)?;
        let output = self
            .submit(CpuOperation::Sha256 { bytes }, SubmissionMode::Durable)
            .await?;
        match output {
            CpuOutput::Sha256(digest) => Ok(digest),
            output => Err(output.mismatch("sha256")),
        }
    }

    pub async fn try_sha256(&self, bytes: Vec<u8>) -> Result<[u8; 32], ExecutorError> {
        validate_hash_input(&bytes)?;
        let output = self
            .submit(CpuOperation::Sha256 { bytes }, SubmissionMode::Try)
            .await?;
        match output {
            CpuOutput::Sha256(digest) => Ok(digest),
            output => Err(output.mismatch("sha256")),
        }
    }

    pub async fn start_sha256_session_request(&self) -> Result<Sha256Session, ExecutorError> {
        self.start_sha256_session(SubmissionMode::Try).await
    }

    pub async fn start_sha256_session_durable(&self) -> Result<Sha256Session, ExecutorError> {
        self.start_sha256_session(SubmissionMode::Durable).await
    }

    async fn start_sha256_session(
        &self,
        mode: SubmissionMode,
    ) -> Result<Sha256Session, ExecutorError> {
        match self.submit(CpuOperation::StartSha256Session, mode).await? {
            CpuOutput::Sha256SessionStarted(session) => Ok(session),
            output => Err(output.mismatch("start_sha256_session")),
        }
    }

    pub async fn update_sha256_session_request(
        &self,
        session: Sha256Session,
        bytes: Vec<u8>,
    ) -> Result<(Sha256Session, Vec<u8>), ExecutorError> {
        self.update_sha256_session(session, bytes, SubmissionMode::Try)
            .await
    }

    pub async fn update_sha256_session_durable(
        &self,
        session: Sha256Session,
        bytes: Vec<u8>,
    ) -> Result<(Sha256Session, Vec<u8>), ExecutorError> {
        self.update_sha256_session(session, bytes, SubmissionMode::Durable)
            .await
    }

    async fn update_sha256_session(
        &self,
        session: Sha256Session,
        bytes: Vec<u8>,
        mode: SubmissionMode,
    ) -> Result<(Sha256Session, Vec<u8>), ExecutorError> {
        validate_hash_input(&bytes)?;
        match self
            .submit(CpuOperation::UpdateSha256Session { session, bytes }, mode)
            .await?
        {
            CpuOutput::Sha256SessionUpdated { session, bytes } => Ok((session, bytes)),
            output => Err(output.mismatch("update_sha256_session")),
        }
    }

    pub async fn finish_sha256_session_request(
        &self,
        session: Sha256Session,
    ) -> Result<String, ExecutorError> {
        self.finish_sha256_session(session, SubmissionMode::Try)
            .await
    }

    pub async fn finish_sha256_session_durable(
        &self,
        session: Sha256Session,
    ) -> Result<String, ExecutorError> {
        self.finish_sha256_session(session, SubmissionMode::Durable)
            .await
    }

    async fn finish_sha256_session(
        &self,
        session: Sha256Session,
        mode: SubmissionMode,
    ) -> Result<String, ExecutorError> {
        match self
            .submit(CpuOperation::FinishSha256Session { session }, mode)
            .await?
        {
            CpuOutput::Sha256SessionFinished(digest) => Ok(digest),
            output => Err(output.mismatch("finish_sha256_session")),
        }
    }

    pub async fn validate_json_durable(&self, bytes: Vec<u8>) -> Result<Vec<u8>, ExecutorError> {
        if bytes.is_empty() || bytes.len() > MAX_JSON_INPUT_BYTES {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "validate_json",
                "JSON input must contain 1..=1048576 bytes",
            ));
        }
        match self
            .submit(
                CpuOperation::ValidateJson { bytes },
                SubmissionMode::Durable,
            )
            .await?
        {
            CpuOutput::JsonValidated(bytes) => Ok(bytes),
            output => Err(output.mismatch("validate_json")),
        }
    }

    pub async fn parse_control_request(
        &self,
        kind: ControlRequestKind,
        bytes: Vec<u8>,
    ) -> Result<ParsedControlRequest, ExecutorError> {
        if bytes.is_empty() || bytes.len() > MAX_JSON_INPUT_BYTES {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "parse_control_request",
                "control JSON must contain 1..=1048576 bytes",
            ));
        }
        match self
            .submit(
                CpuOperation::ParseControlRequest { kind, bytes },
                SubmissionMode::Try,
            )
            .await?
        {
            CpuOutput::ControlRequestParsed(parsed) => Ok(parsed),
            output => Err(output.mismatch("parse_control_request")),
        }
    }

    pub async fn serialize_control_response(
        &self,
        response: ControlResponse,
    ) -> Result<Vec<u8>, ExecutorError> {
        match self
            .submit(
                CpuOperation::SerializeControlResponse {
                    response: Box::new(response),
                },
                SubmissionMode::Try,
            )
            .await?
        {
            CpuOutput::ControlResponseSerialized(bytes) => Ok(bytes),
            output => Err(output.mismatch("serialize_control_response")),
        }
    }

    pub async fn serialize_backup_metadata(
        &self,
        metadata: serde_json::Value,
    ) -> Result<String, ExecutorError> {
        match self
            .submit(
                CpuOperation::SerializeBackupMetadata { metadata },
                SubmissionMode::Try,
            )
            .await?
        {
            CpuOutput::BackupMetadataSerialized(metadata) => Ok(metadata),
            output => Err(output.mismatch("serialize_backup_metadata")),
        }
    }

    pub async fn decode_place_identity(
        &self,
        place_id: String,
    ) -> Result<PlaceIdentityDto, ExecutorError> {
        if place_id.is_empty() || place_id.len() > 4096 {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "decode_place_identity",
                "placeId must contain 1..=4096 bytes",
            ));
        }
        match self
            .submit(
                CpuOperation::DecodePlaceIdentity { place_id },
                SubmissionMode::Try,
            )
            .await?
        {
            CpuOutput::PlaceIdentityDecoded(identity) => Ok(identity),
            output => Err(output.mismatch("decode_place_identity")),
        }
    }

    pub async fn build_place_summaries(
        &self,
        rows: Vec<crate::database::operations::PlaceRecord>,
    ) -> Result<Vec<crate::models::PlaceSummary>, ExecutorError> {
        if rows.len() > 201 {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "build_place_summaries",
                "place summary page exceeds 201 rows",
            ));
        }
        match self
            .submit(
                CpuOperation::BuildPlaceSummaries { rows },
                SubmissionMode::Try,
            )
            .await?
        {
            CpuOutput::PlaceSummariesBuilt(summaries) => Ok(summaries),
            output => Err(output.mismatch("build_place_summaries")),
        }
    }

    async fn parse_metadata_json_durable(
        &self,
        bytes: Vec<u8>,
        kind: MetadataJsonKind,
    ) -> Result<CpuOutput, ExecutorError> {
        if bytes.is_empty() || bytes.len() > MAX_METADATA_JSON_INPUT_BYTES {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "parse_metadata_json",
                "metadata JSON input must contain 1..=4194304 bytes",
            ));
        }
        self.submit(
            CpuOperation::ParseMetadataJson { bytes, kind },
            SubmissionMode::Durable,
        )
        .await
    }

    pub async fn parse_supplemental_metadata_durable(
        &self,
        bytes: Vec<u8>,
    ) -> Result<ParsedSupplementalMetadata, ExecutorError> {
        match self
            .parse_metadata_json_durable(bytes, MetadataJsonKind::Supplemental)
            .await?
        {
            CpuOutput::SupplementalMetadataParsed(value) => Ok(value),
            output => Err(output.mismatch("parse_supplemental_metadata")),
        }
    }

    pub async fn parse_exif_metadata_durable(
        &self,
        bytes: Vec<u8>,
    ) -> Result<ParsedExifMetadata, ExecutorError> {
        match self
            .parse_metadata_json_durable(bytes, MetadataJsonKind::ExifTool)
            .await?
        {
            CpuOutput::ExifMetadataParsed(value) => Ok(value),
            output => Err(output.mismatch("parse_exif_metadata")),
        }
    }

    pub async fn parse_ffprobe_metadata_durable(
        &self,
        bytes: Vec<u8>,
    ) -> Result<ParsedFfprobeMetadata, ExecutorError> {
        match self
            .parse_metadata_json_durable(bytes, MetadataJsonKind::Ffprobe)
            .await?
        {
            CpuOutput::FfprobeMetadataParsed(value) => Ok(value),
            output => Err(output.mismatch("parse_ffprobe_metadata")),
        }
    }

    pub async fn render_request_log_payload_request(
        &self,
        bytes: Vec<u8>,
        maximum_bytes: usize,
        truncated: bool,
    ) -> Result<String, ExecutorError> {
        if maximum_bytes == 0 || maximum_bytes > MAX_JSON_INPUT_BYTES || bytes.len() > maximum_bytes
        {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "render_request_log_payload",
                "request-log payload bounds are invalid",
            ));
        }
        match self
            .submit(
                CpuOperation::RenderRequestLogPayload {
                    bytes,
                    maximum_bytes,
                    truncated,
                },
                SubmissionMode::Try,
            )
            .await?
        {
            CpuOutput::RequestLogPayload(payload) => Ok(payload),
            output => Err(output.mismatch("render_request_log_payload")),
        }
    }

    pub async fn auth_attempt_digests_durable(
        &self,
        source: String,
        identity: String,
    ) -> Result<([u8; 32], [u8; 32]), ExecutorError> {
        let source = source.into_bytes();
        let identity = identity.to_lowercase().into_bytes();
        if source.len() > MAX_AUTH_SOURCE_BYTES || identity.len() > MAX_AUTH_IDENTITY_BYTES {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "auth_attempt_digests",
                format!(
                    "authentication source/identity exceeds {MAX_AUTH_SOURCE_BYTES}/{MAX_AUTH_IDENTITY_BYTES} bytes"
                ),
            ));
        }
        match self
            .submit(
                CpuOperation::AuthAttemptDigests { source, identity },
                SubmissionMode::Durable,
            )
            .await?
        {
            CpuOutput::AuthAttemptDigests { source, identity } => Ok((source, identity)),
            output => Err(output.mismatch("auth_attempt_digests")),
        }
    }

    pub(crate) async fn hash_password_durable(
        &self,
        password: String,
    ) -> Result<String, ExecutorError> {
        validate_password(&password, "hash_password")?;
        match self
            .submit(
                CpuOperation::HashPassword { password },
                SubmissionMode::Durable,
            )
            .await?
        {
            CpuOutput::PasswordHash(hash) => Ok(hash),
            output => Err(output.mismatch("hash_password")),
        }
    }

    pub(crate) async fn verify_password_durable(
        &self,
        password: String,
        hash: Option<String>,
        dummy_hash: String,
    ) -> Result<bool, ExecutorError> {
        validate_password(&password, "verify_password")?;
        if hash
            .as_ref()
            .is_some_and(|hash| hash.len() > MAX_PASSWORD_HASH_BYTES)
            || dummy_hash.len() > MAX_PASSWORD_HASH_BYTES
        {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "verify_password",
                "encoded password hash exceeds 512 bytes",
            ));
        }
        match self
            .submit(
                CpuOperation::VerifyPassword {
                    password,
                    hash,
                    dummy_hash,
                },
                SubmissionMode::Durable,
            )
            .await?
        {
            CpuOutput::PasswordVerified(verified) => Ok(verified),
            output => Err(output.mismatch("verify_password")),
        }
    }

    pub(crate) async fn prepare_llm_cron_update_durable(
        &self,
        identity: ConfigFileIdentity,
        contents: String,
        config_field_name: &'static str,
        feature_name: &'static str,
        cron_expression: String,
    ) -> Result<(String, crate::config::Config), ExecutorError> {
        if contents.len() as u64 > crate::runtime::config_bootstrap::MAX_CONFIG_BYTES {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "prepare_llm_cron_update",
                "config exceeds one mebibyte",
            ));
        }
        match self
            .submit(
                CpuOperation::PrepareLlmCronUpdate {
                    identity,
                    contents,
                    config_field_name,
                    feature_name,
                    cron_expression,
                },
                SubmissionMode::Durable,
            )
            .await?
        {
            CpuOutput::LlmCronUpdate { contents, config } => Ok((contents, *config)),
            output => Err(output.mismatch("prepare_llm_cron_update")),
        }
    }

    pub(crate) async fn prepare_admin_password_reset_update_durable(
        &self,
        identity: ConfigFileIdentity,
        contents: String,
    ) -> Result<(String, crate::config::Config), ExecutorError> {
        match self
            .submit(
                CpuOperation::PrepareAdminPasswordResetUpdate { identity, contents },
                SubmissionMode::Durable,
            )
            .await?
        {
            CpuOutput::AdminPasswordResetUpdate { contents, config } => Ok((contents, *config)),
            output => Err(output.mismatch("prepare_admin_password_reset_update")),
        }
    }

    pub async fn initialize_reverse_geocoder_durable(&self) -> Result<usize, ExecutorError> {
        if let Some(snapshot) = self.reverse_geocoder.get() {
            return Ok(snapshot.record_count());
        }
        let snapshot = match self
            .submit(
                CpuOperation::InitializeReverseGeocoder,
                SubmissionMode::Durable,
            )
            .await?
        {
            CpuOutput::ReverseGeocoderInitialized(snapshot) => snapshot,
            output => return Err(output.mismatch("initialize_reverse_geocoder")),
        };
        let record_count = snapshot.record_count();
        let _ = self.reverse_geocoder.set(snapshot);
        Ok(record_count)
    }

    pub async fn compute_cron_catch_up_page_durable(
        &self,
        cron_expression: String,
        after: DateTime<Utc>,
        now: DateTime<Utc>,
        timezone: Tz,
        maximum_occurrences: u16,
    ) -> Result<CronCatchUpPage, ExecutorError> {
        if cron_expression.is_empty()
            || cron_expression.len() > 256
            || maximum_occurrences == 0
            || maximum_occurrences > 256
        {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "compute_cron_catch_up_page",
                "cron catch-up requires a 1..=256 byte expression and 1..=256 occurrences",
            ));
        }
        match self
            .submit(
                CpuOperation::ComputeCronCatchUpPage {
                    cron_expression,
                    after,
                    now,
                    timezone,
                    maximum_occurrences,
                },
                SubmissionMode::Durable,
            )
            .await?
        {
            CpuOutput::CronCatchUpPage(page) => Ok(page),
            output => Err(output.mismatch("compute_cron_catch_up_page")),
        }
    }

    pub async fn derive_media_location_request(
        &self,
        latitude: Option<f64>,
        longitude: Option<f64>,
    ) -> Result<DerivedMediaLocation, ExecutorError> {
        self.derive_media_location(latitude, longitude, SubmissionMode::Try)
            .await
    }

    pub async fn derive_media_location_durable(
        &self,
        latitude: Option<f64>,
        longitude: Option<f64>,
    ) -> Result<DerivedMediaLocation, ExecutorError> {
        self.derive_media_location(latitude, longitude, SubmissionMode::Durable)
            .await
    }

    pub async fn compare_deduplicate_page(
        &self,
        page: crate::processor::deduplicator::DeduplicateComparisonPage,
    ) -> Result<crate::processor::deduplicator::DeduplicateCpuResult, ExecutorError> {
        match self
            .submit(
                CpuOperation::CompareDeduplicatePage { page },
                SubmissionMode::Durable,
            )
            .await?
        {
            CpuOutput::DeduplicateCpuResult(result) => Ok(result),
            output => Err(output.mismatch("compare_deduplicate_page")),
        }
    }

    pub async fn measure_deduplicate_group_page(
        &self,
        page: crate::processor::deduplicator::DeduplicateGroupPage,
    ) -> Result<crate::processor::deduplicator::DeduplicateCpuResult, ExecutorError> {
        match self
            .submit(
                CpuOperation::MeasureDeduplicateGroupPage { page },
                SubmissionMode::Durable,
            )
            .await?
        {
            CpuOutput::DeduplicateCpuResult(result) => Ok(result),
            output => Err(output.mismatch("measure_deduplicate_group_page")),
        }
    }

    pub async fn compare_face_group_page(
        &self,
        page: crate::processor::face_detection::FaceComparisonPage,
    ) -> Result<crate::processor::face_detection::FaceGroupCpuResult, ExecutorError> {
        match self
            .submit(
                CpuOperation::CompareFaceGroupPage { page },
                SubmissionMode::Durable,
            )
            .await?
        {
            CpuOutput::FaceGroupCpuResult(result) => Ok(result),
            output => Err(output.mismatch("compare_face_group_page")),
        }
    }

    pub async fn reduce_face_representative_page(
        &self,
        page: crate::processor::face_detection::FaceRepresentativeReductionPage,
    ) -> Result<crate::processor::face_detection::FaceGroupCpuResult, ExecutorError> {
        match self
            .submit(
                CpuOperation::ReduceFaceRepresentativePage { page },
                SubmissionMode::Durable,
            )
            .await?
        {
            CpuOutput::FaceGroupCpuResult(result) => Ok(result),
            output => Err(output.mismatch("reduce_face_representative_page")),
        }
    }

    async fn derive_media_location(
        &self,
        latitude: Option<f64>,
        longitude: Option<f64>,
        mode: SubmissionMode,
    ) -> Result<DerivedMediaLocation, ExecutorError> {
        if latitude.is_some_and(|value| !value.is_finite() || !(-90.0..=90.0).contains(&value))
            || longitude
                .is_some_and(|value| !value.is_finite() || !(-180.0..=180.0).contains(&value))
        {
            return Err(ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "derive_media_location",
                "GPS coordinates are invalid",
            ));
        }
        let geocoder = self.reverse_geocoder.get().cloned();
        if latitude.zip(longitude).is_some() && geocoder.is_none() {
            return Err(ExecutorError::new(
                ExecutorErrorKind::Internal,
                "derive_media_location",
                "reverse geocoder was not initialized before request admission",
            ));
        }
        match self
            .submit(
                CpuOperation::DeriveMediaLocation {
                    geocoder,
                    latitude,
                    longitude,
                },
                mode,
            )
            .await?
        {
            CpuOutput::MediaLocation(location) => Ok(location),
            output => Err(output.mismatch("derive_media_location")),
        }
    }

    pub(crate) async fn supervise_child_process_durable(
        &self,
        spec: ChildProcessSpec,
    ) -> Result<ChildProcessCompletion, ExecutorError> {
        match self
            .submit(
                CpuOperation::SuperviseChildProcess { spec },
                SubmissionMode::Durable,
            )
            .await?
        {
            CpuOutput::ChildProcessSupervised(completion) => Ok(completion),
            output => Err(output.mismatch("supervise_child_process")),
        }
    }

    async fn submit(
        &self,
        operation: CpuOperation,
        mode: SubmissionMode,
    ) -> Result<CpuOutput, ExecutorError> {
        let operation_name = operation.name();
        let (reply, response) = oneshot::channel();
        self.ingress
            .submit_cpu(CpuCommand::new(operation, reply), mode, operation_name)?;
        response
            .await
            .map_err(|_| ExecutorError::shutting_down(operation_name))?
    }
}

fn validate_hash_input(bytes: &[u8]) -> Result<(), ExecutorError> {
    if bytes.len() <= MAX_HASH_INPUT_BYTES {
        return Ok(());
    }
    Err(ExecutorError::new(
        ExecutorErrorKind::InvalidInput,
        "sha256",
        format!(
            "hash input is {} bytes; maximum is {MAX_HASH_INPUT_BYTES}",
            bytes.len()
        ),
    ))
}

fn validate_password(password: &str, operation: &'static str) -> Result<(), ExecutorError> {
    if password.len() <= crate::auth::MAX_AUTH_PASSWORD_BYTES {
        return Ok(());
    }
    Err(ExecutorError::new(
        ExecutorErrorKind::InvalidInput,
        operation,
        format!(
            "password is {} bytes; maximum is {}",
            password.len(),
            crate::auth::MAX_AUTH_PASSWORD_BYTES
        ),
    ))
}

fn decode_place_identity(place_id: &str) -> Result<PlaceIdentityDto, String> {
    let bytes = URL_SAFE_NO_PAD
        .decode(place_id)
        .map_err(|_| "placeId is invalid".to_string())?;
    let identity = serde_json::from_slice::<PlaceIdentityDto>(&bytes)
        .map_err(|_| "placeId is invalid".to_string())?;
    if identity.city.trim().is_empty() || identity.country.trim().is_empty() {
        return Err("placeId is invalid".to_string());
    }
    Ok(identity)
}

fn build_place_summaries(
    rows: Vec<crate::database::operations::PlaceRecord>,
) -> Result<Vec<crate::models::PlaceSummary>, String> {
    rows.into_iter()
        .map(|row| {
            let identity = PlaceIdentityDto {
                city: row.city,
                state: row.state,
                country: row.country,
            };
            let bytes = serde_json::to_vec(&identity).map_err(|error| error.to_string())?;
            let place_id = URL_SAFE_NO_PAD.encode(bytes);
            Ok(crate::models::PlaceSummary {
                place_id,
                city: identity.city,
                state: identity.state,
                country: identity.country,
                media_count: row.media_count,
            })
        })
        .collect()
}

pub(crate) fn spawn_cpu_workers(
    worker_count: usize,
    receiver: Receiver<CpuCommand>,
    capacity_wake: std::sync::Arc<Notify>,
) -> Result<Vec<JoinHandle<()>>, std::io::Error> {
    let mut workers = Vec::new();
    workers.try_reserve_exact(worker_count).map_err(|error| {
        std::io::Error::other(format!("failed to reserve CPU worker handles: {error}"))
    })?;
    for worker_index in 0..worker_count {
        let receiver = receiver.clone();
        let capacity_wake = std::sync::Arc::clone(&capacity_wake);
        workers.push(
            std::thread::Builder::new()
                .name(format!("momento-cpu-{worker_index}"))
                .stack_size(crate::runtime::WORKER_STACK_BYTES as usize)
                .spawn(move || run_worker(receiver, capacity_wake))?,
        );
    }
    Ok(workers)
}

fn run_worker(receiver: Receiver<CpuCommand>, capacity_wake: std::sync::Arc<Notify>) {
    while let Ok(command) = receiver.recv() {
        capacity_wake.notify_one();
        let operation_name = command.operation.name();
        let operation_result = catch_unwind(AssertUnwindSafe(|| execute(command.operation)))
            .unwrap_or_else(|_| {
                Err(ExecutorError::new(
                    ExecutorErrorKind::WorkerPanic,
                    operation_name,
                    "CPU operation panicked",
                ))
            });
        let _ = command.reply.send(operation_result);
    }
}

fn execute(operation: CpuOperation) -> Result<CpuOutput, ExecutorError> {
    let _operation_spec = operation.spec();
    match operation {
        CpuOperation::Probe { sequence } => Ok(CpuOutput::Probe {
            sequence,
            thread_name: std::thread::current()
                .name()
                .unwrap_or("unnamed")
                .to_string(),
        }),
        CpuOperation::Sha256 { bytes } => Ok(CpuOutput::Sha256(Sha256::digest(bytes).into())),
        CpuOperation::StartSha256Session => Ok(CpuOutput::Sha256SessionStarted(Sha256Session {
            hasher: Sha256::new(),
        })),
        CpuOperation::UpdateSha256Session { mut session, bytes } => {
            session.hasher.update(&bytes);
            Ok(CpuOutput::Sha256SessionUpdated { session, bytes })
        }
        CpuOperation::FinishSha256Session { session } => Ok(CpuOutput::Sha256SessionFinished(
            format!("{:x}", session.hasher.finalize()),
        )),
        CpuOperation::ValidateJson { bytes } => super::bounded_json::parse_bounded_json(&bytes)
            .map(|_| CpuOutput::JsonValidated(bytes))
            .map_err(|error| {
                ExecutorError::new(
                    ExecutorErrorKind::InvalidInput,
                    "validate_json",
                    error.to_string(),
                )
            }),
        CpuOperation::ParseControlRequest { kind, bytes } => Ok(CpuOutput::ControlRequestParsed(
            super::control_json::parse_control_request(kind, &bytes),
        )),
        CpuOperation::SerializeControlResponse { response } => serde_json::to_vec(&response)
            .map_err(|error| {
                ExecutorError::new(
                    ExecutorErrorKind::Internal,
                    "serialize_control_response",
                    error.to_string(),
                )
            })
            .and_then(|bytes| {
                if bytes.len() > MAX_JSON_RESPONSE_BYTES {
                    return Err(ExecutorError::new(
                        ExecutorErrorKind::InvalidInput,
                        "serialize_control_response",
                        "JSON response exceeds four mebibytes",
                    ));
                }
                Ok(CpuOutput::ControlResponseSerialized(bytes))
            }),
        CpuOperation::SerializeBackupMetadata { metadata } => serde_json::to_string(&metadata)
            .map_err(|error| {
                ExecutorError::new(
                    ExecutorErrorKind::Internal,
                    "serialize_backup_metadata",
                    error.to_string(),
                )
            })
            .and_then(|metadata| {
                if metadata.len() > MAX_JSON_INPUT_BYTES {
                    return Err(ExecutorError::new(
                        ExecutorErrorKind::InvalidInput,
                        "serialize_backup_metadata",
                        "backup metadata exceeds one mebibyte",
                    ));
                }
                Ok(CpuOutput::BackupMetadataSerialized(metadata))
            }),
        CpuOperation::DecodePlaceIdentity { place_id } => decode_place_identity(&place_id)
            .map(CpuOutput::PlaceIdentityDecoded)
            .map_err(|error| {
                ExecutorError::new(
                    ExecutorErrorKind::InvalidInput,
                    "decode_place_identity",
                    error,
                )
            }),
        CpuOperation::BuildPlaceSummaries { rows } => build_place_summaries(rows)
            .map(CpuOutput::PlaceSummariesBuilt)
            .map_err(|error| {
                ExecutorError::new(
                    ExecutorErrorKind::InvalidInput,
                    "build_place_summaries",
                    error,
                )
            }),
        CpuOperation::ParseMetadataJson { bytes, kind } => {
            let parsed = match kind {
                MetadataJsonKind::Supplemental => {
                    super::bounded_json::parse_supplemental_metadata(&bytes)
                        .map(CpuOutput::SupplementalMetadataParsed)
                }
                MetadataJsonKind::ExifTool => super::bounded_json::parse_exif_metadata(&bytes)
                    .map(CpuOutput::ExifMetadataParsed),
                MetadataJsonKind::Ffprobe => super::bounded_json::parse_ffprobe_metadata(&bytes)
                    .map(CpuOutput::FfprobeMetadataParsed),
            };
            parsed.map_err(|error| {
                ExecutorError::new(
                    ExecutorErrorKind::InvalidInput,
                    "parse_metadata_json",
                    error,
                )
            })
        }
        CpuOperation::RenderRequestLogPayload {
            bytes,
            maximum_bytes,
            truncated,
        } => Ok(CpuOutput::RequestLogPayload(
            crate::logging::render_captured_request_payload(&bytes, maximum_bytes, truncated),
        )),
        CpuOperation::AuthAttemptDigests { source, identity } => {
            let mut source_hasher = Sha256::new();
            source_hasher.update(b"source\0");
            source_hasher.update(source);
            let mut identity_hasher = Sha256::new();
            identity_hasher.update(b"identity\0");
            identity_hasher.update(identity);
            Ok(CpuOutput::AuthAttemptDigests {
                source: source_hasher.finalize().into(),
                identity: identity_hasher.finalize().into(),
            })
        }
        CpuOperation::HashPassword { password } => crate::auth::hash_password(&password)
            .map(CpuOutput::PasswordHash)
            .map_err(|error| {
                ExecutorError::new(
                    ExecutorErrorKind::Internal,
                    "hash_password",
                    error.to_string(),
                )
            }),
        CpuOperation::VerifyPassword {
            password,
            hash,
            dummy_hash,
        } => Ok(CpuOutput::PasswordVerified(
            crate::auth::verify_password_or_dummy(&password, hash.as_deref(), &dummy_hash),
        )),
        CpuOperation::PrepareLlmCronUpdate {
            identity,
            contents,
            config_field_name,
            feature_name,
            cron_expression,
        } => crate::config::prepare_llm_cron_update(
            &identity.canonical_path,
            &contents,
            config_field_name,
            feature_name,
            &cron_expression,
        )
        .map(|(contents, config)| CpuOutput::LlmCronUpdate {
            contents,
            config: Box::new(config),
        })
        .map_err(|error| {
            ExecutorError::new(
                ExecutorErrorKind::InvalidInput,
                "prepare_llm_cron_update",
                error.to_string(),
            )
        }),
        CpuOperation::PrepareAdminPasswordResetUpdate { identity, contents } => {
            crate::config::prepare_admin_password_reset_update(&identity.canonical_path, &contents)
                .map(|(contents, config)| CpuOutput::AdminPasswordResetUpdate {
                    contents,
                    config: Box::new(config),
                })
                .map_err(|error| {
                    ExecutorError::new(
                        ExecutorErrorKind::InvalidInput,
                        "prepare_admin_password_reset_update",
                        error.to_string(),
                    )
                })
        }
        CpuOperation::InitializeReverseGeocoder => ReverseGeocoderSnapshot::from_embedded()
            .map(CpuOutput::ReverseGeocoderInitialized)
            .map_err(|error| {
                ExecutorError::new(
                    ExecutorErrorKind::Internal,
                    "initialize_reverse_geocoder",
                    error,
                )
            }),
        CpuOperation::ComputeCronCatchUpPage {
            cron_expression,
            after,
            now,
            timezone,
            maximum_occurrences,
        } => {
            let mut next =
                crate::cronjob::next_scheduled_at(&cron_expression, "deduplicate", after, timezone)
                    .map_err(|error| {
                        ExecutorError::new(
                            ExecutorErrorKind::InvalidInput,
                            "compute_cron_catch_up_page",
                            error.to_string(),
                        )
                    })?;
            let mut latest_due = None;
            for _ in 0..maximum_occurrences {
                if next > now {
                    break;
                }
                latest_due = Some(next);
                next = crate::cronjob::next_scheduled_at(
                    &cron_expression,
                    "deduplicate",
                    next,
                    timezone,
                )
                .map_err(|error| {
                    ExecutorError::new(
                        ExecutorErrorKind::InvalidInput,
                        "compute_cron_catch_up_page",
                        error.to_string(),
                    )
                })?;
            }
            Ok(CpuOutput::CronCatchUpPage(CronCatchUpPage {
                latest_due,
                next,
                continuation_required: next <= now,
            }))
        }
        CpuOperation::DeriveMediaLocation {
            geocoder,
            latitude,
            longitude,
        } => {
            let geohash = match (latitude, longitude) {
                (Some(latitude), Some(longitude)) => {
                    crate::processor::media_processor::calculate_geohash(latitude, longitude)
                }
                _ => None,
            };
            let location = match (latitude, longitude) {
                (Some(latitude), Some(longitude)) => {
                    geocoder.and_then(|geocoder| geocoder.search(latitude, longitude))
                }
                _ => None,
            };
            let (city, state, country) = location
                .map(|location| (Some(location.city), location.state, Some(location.country)))
                .unwrap_or((None, None, None));
            Ok(CpuOutput::MediaLocation(DerivedMediaLocation {
                geohash,
                city,
                state,
                country,
            }))
        }
        CpuOperation::CompareDeduplicatePage { page } => {
            crate::processor::deduplicator::compare_page(page)
                .map(CpuOutput::DeduplicateCpuResult)
                .map_err(|error| {
                    ExecutorError::new(
                        ExecutorErrorKind::InvalidInput,
                        "compare_deduplicate_page",
                        error,
                    )
                })
        }
        CpuOperation::MeasureDeduplicateGroupPage { page } => {
            crate::processor::deduplicator::measure_group_page(page)
                .map(CpuOutput::DeduplicateCpuResult)
                .map_err(|error| {
                    ExecutorError::new(
                        ExecutorErrorKind::InvalidInput,
                        "measure_deduplicate_group_page",
                        error,
                    )
                })
        }
        CpuOperation::CompareFaceGroupPage { page } => {
            crate::processor::face_detection::compare_group_page(page)
                .map(CpuOutput::FaceGroupCpuResult)
                .map_err(|error| {
                    ExecutorError::new(
                        ExecutorErrorKind::InvalidInput,
                        "compare_face_group_page",
                        error,
                    )
                })
        }
        CpuOperation::ReduceFaceRepresentativePage { page } => {
            crate::processor::face_detection::reduce_representative_page(page)
                .map(CpuOutput::FaceGroupCpuResult)
                .map_err(|error| {
                    ExecutorError::new(
                        ExecutorErrorKind::InvalidInput,
                        "reduce_face_representative_page",
                        error,
                    )
                })
        }
        CpuOperation::SuperviseChildProcess { spec } => {
            Ok(CpuOutput::ChildProcessSupervised(spec.run()))
        }
    }
}
