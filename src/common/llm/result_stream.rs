use super::result_payload::{
    decode_payload, normalized_heap_bytes, ClassificationPayload, DecodedResultPayload,
    FacePayload, ImageAestheticsPayload, ImageClusteringPayload,
};
use super::{
    decode_result_record_header, decode_result_record_parts, is_valid_job_id, ResultRecord,
    ResultRecordHeader, ResultRecordKind, MAX_LLM_RESULT_CONTINUATIONS_PER_VALUE,
    MAX_LLM_RESULT_RECORDS, MAX_NORMALIZED_RESULT_RECORD_BYTES, RESULT_RECORD_HEADER_BYTES,
};
use serde::{Deserialize, Serialize};

pub const MAX_LLM_RESULT_BYTES: u64 = 1024 * 1024 * 1024;
pub const MAX_LLM_RESULT_INPUTS: usize = 1024;
pub const RESULT_RECORDS_ENCODING: &str = "momento-result-records-v1";
pub const MAX_LLM_MODEL_TYPE_BYTES: usize = 63;
pub const MAX_LLM_MODEL_VERSION_BYTES: usize = 255;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResultManifest {
    pub job_id: String,
    pub media_id: i64,
    pub task: String,
    pub attempt: u32,
    pub status: ResultStatus,
    pub model_type: Option<String>,
    pub model_version: Option<String>,
    pub encoding: String,
    pub record_count: u32,
    pub byte_size: u64,
    pub content_hash: String,
}

impl ResultManifest {
    pub fn validate(&self) -> Result<(), String> {
        if !is_valid_job_id(&self.job_id) {
            return Err("result manifest job ID is invalid".to_string());
        }
        if self.media_id <= 0 {
            return Err("result manifest media ID must be positive".to_string());
        }
        ResultTask::parse(&self.task)?;
        if self.encoding != RESULT_RECORDS_ENCODING {
            return Err("result manifest encoding is unsupported".to_string());
        }
        if self.record_count == 0 || self.record_count > MAX_LLM_RESULT_RECORDS {
            return Err("result manifest record count is outside its bound".to_string());
        }
        let minimum_bytes = u64::from(self.record_count)
            .checked_mul(super::RESULT_RECORD_HEADER_BYTES as u64)
            .ok_or_else(|| "result manifest minimum byte size overflowed".to_string())?;
        if self.byte_size < minimum_bytes || self.byte_size > MAX_LLM_RESULT_BYTES {
            return Err("result manifest byte size is outside its bound".to_string());
        }
        if self.content_hash.len() != 64
            || !self
                .content_hash
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err("result manifest content hash must be SHA-256 hexadecimal".to_string());
        }
        match self.status {
            ResultStatus::Completed => {
                validate_model_field(
                    self.model_type.as_deref(),
                    MAX_LLM_MODEL_TYPE_BYTES,
                    "model type",
                )?;
                validate_model_field(
                    self.model_version.as_deref(),
                    MAX_LLM_MODEL_VERSION_BYTES,
                    "model version",
                )?;
            }
            ResultStatus::Failed if self.model_type.is_some() || self.model_version.is_some() => {
                return Err("failed result manifest must not contain model metadata".to_string());
            }
            ResultStatus::Failed => {}
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResultInputCorrelation {
    pub sequence: u32,
    pub frame_timestamp_ms: Option<i64>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResultStatus {
    Completed,
    Failed,
}

fn validate_model_field(
    value: Option<&str>,
    maximum_bytes: usize,
    field: &str,
) -> Result<(), String> {
    let value = value.ok_or_else(|| format!("completed result manifest requires {field}"))?;
    if value.is_empty() || value.len() > maximum_bytes {
        return Err(format!("result manifest {field} is outside its byte bound"));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResultTask {
    Ocr,
    ImageTagging,
    ImageClustering,
    ImageAesthetics,
    FaceDetection,
    ScreenshotDetection,
    DocumentDetection,
}

impl ResultTask {
    fn parse(task: &str) -> Result<Self, String> {
        match task {
            "ocr" => Ok(Self::Ocr),
            "image_tagging" => Ok(Self::ImageTagging),
            "image_clustering" => Ok(Self::ImageClustering),
            "image_aesthetics" => Ok(Self::ImageAesthetics),
            "face_detection" => Ok(Self::FaceDetection),
            "screenshot_detection" => Ok(Self::ScreenshotDetection),
            "document_detection" => Ok(Self::DocumentDetection),
            _ => Err("result task is unknown".to_string()),
        }
    }

    fn primary_kind(self) -> ResultRecordKind {
        match self {
            Self::Ocr => ResultRecordKind::OcrText,
            Self::ImageTagging => ResultRecordKind::ImageTags,
            Self::ImageClustering => ResultRecordKind::ImageClustering,
            Self::ImageAesthetics => ResultRecordKind::ImageAesthetics,
            Self::FaceDetection => ResultRecordKind::Face,
            Self::ScreenshotDetection => ResultRecordKind::ScreenshotDetection,
            Self::DocumentDetection => ResultRecordKind::DocumentDetection,
        }
    }

    fn continuation_kind(self) -> Option<ResultRecordKind> {
        match self {
            Self::Ocr => Some(ResultRecordKind::OcrTextContinuation),
            Self::ImageTagging => Some(ResultRecordKind::ImageTagsContinuation),
            _ => None,
        }
    }

    fn permits_empty_input(self) -> bool {
        self == Self::FaceDetection
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InputState {
    AwaitingStart,
    AwaitingPrimary,
    AcceptingValues,
}

pub struct ResultRecordStreamValidator {
    task: ResultTask,
    status: ResultStatus,
    inputs: Vec<ResultInputCorrelation>,
    declared_record_count: u32,
    next_record_sequence: u32,
    input_index: usize,
    input_state: InputState,
    continuation_count: u8,
    aggregate_normalized_bytes: usize,
    failed_record_seen: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ValidatedResultStream {
    pub status: ResultStatus,
    pub inputs: Vec<ValidatedResultInput>,
    pub failure: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ValidatedResultInput {
    pub sequence: u32,
    pub frame_timestamp_ms: Option<i64>,
    pub value: ValidatedResultValue,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ValidatedResultValue {
    Ocr(String),
    ImageTags(Vec<String>),
    ImageClustering(ImageClusteringPayload),
    ImageAesthetics(ImageAestheticsPayload),
    Faces(Vec<FacePayload>),
    ScreenshotDetection(ClassificationPayload),
    DocumentDetection(ClassificationPayload),
}

pub struct ResultRecordCollector {
    validator: ResultRecordStreamValidator,
    task: ResultTask,
    status: ResultStatus,
    current: Option<ResultInputAccumulator>,
    inputs: Vec<ValidatedResultInput>,
    failure: Option<String>,
}

struct ResultInputAccumulator {
    sequence: u32,
    frame_timestamp_ms: Option<i64>,
    text: String,
    tags: Vec<String>,
    clustering: Option<ImageClusteringPayload>,
    aesthetics: Option<ImageAestheticsPayload>,
    faces: Vec<FacePayload>,
    classification: Option<ClassificationPayload>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedResultRecord {
    pub kind: ResultRecordKind,
    pub flags: u16,
    pub record_sequence: u32,
    pub input_sequence: u32,
    pub payload: Vec<u8>,
}

impl OwnedResultRecord {
    pub fn as_borrowed(&self) -> ResultRecord<'_> {
        ResultRecord {
            kind: self.kind,
            flags: self.flags,
            record_sequence: self.record_sequence,
            input_sequence: self.input_sequence,
            payload: &self.payload,
        }
    }
}

pub struct ResultRecordChunkDecoder {
    header_bytes: [u8; RESULT_RECORD_HEADER_BYTES],
    header_length: usize,
    current: Option<PartialResultRecord>,
}

struct PartialResultRecord {
    header: ResultRecordHeader,
    payload: Vec<u8>,
}

impl ResultRecordChunkDecoder {
    pub fn new() -> Self {
        Self {
            header_bytes: [0; RESULT_RECORD_HEADER_BYTES],
            header_length: 0,
            current: None,
        }
    }

    pub fn push<Emit>(&mut self, mut chunk: &[u8], mut emit: Emit) -> Result<(), String>
    where
        Emit: FnMut(OwnedResultRecord) -> Result<(), String>,
    {
        while !chunk.is_empty() {
            if self.current.is_none() {
                let needed = RESULT_RECORD_HEADER_BYTES - self.header_length;
                let copied = needed.min(chunk.len());
                self.header_bytes[self.header_length..self.header_length + copied]
                    .copy_from_slice(&chunk[..copied]);
                self.header_length += copied;
                chunk = &chunk[copied..];
                if self.header_length != RESULT_RECORD_HEADER_BYTES {
                    continue;
                }
                let header = decode_result_record_header(&self.header_bytes)?;
                let mut payload = Vec::new();
                payload
                    .try_reserve_exact(header.payload_length)
                    .map_err(|error| format!("could not reserve result record payload: {error}"))?;
                self.current = Some(PartialResultRecord { header, payload });
                if self
                    .current
                    .as_ref()
                    .is_some_and(|current| current.header.payload_length == 0)
                {
                    self.emit_current(&mut emit)?;
                }
                continue;
            }
            let current = self.current.as_mut().expect("current record exists");
            let needed = current.header.payload_length - current.payload.len();
            let copied = needed.min(chunk.len());
            current.payload.extend_from_slice(&chunk[..copied]);
            chunk = &chunk[copied..];
            if current.payload.len() == current.header.payload_length {
                self.emit_current(&mut emit)?;
            }
        }
        Ok(())
    }

    pub fn finish(self) -> Result<(), String> {
        if self.header_length == 0 && self.current.is_none() {
            Ok(())
        } else {
            Err("result record stream ended with a partial record".to_string())
        }
    }

    fn emit_current<Emit>(&mut self, emit: &mut Emit) -> Result<(), String>
    where
        Emit: FnMut(OwnedResultRecord) -> Result<(), String>,
    {
        let current = self.current.take().expect("current record exists");
        let decoded = decode_result_record_parts(&self.header_bytes, &current.payload)?;
        let owned = OwnedResultRecord {
            kind: decoded.kind,
            flags: decoded.flags,
            record_sequence: decoded.record_sequence,
            input_sequence: decoded.input_sequence,
            payload: current.payload,
        };
        self.header_length = 0;
        self.header_bytes.fill(0);
        emit(owned)
    }
}

impl Default for ResultRecordChunkDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl ResultRecordStreamValidator {
    pub fn new(
        task: &str,
        status: ResultStatus,
        inputs: &[ResultInputCorrelation],
        declared_record_count: u32,
        declared_byte_size: u64,
    ) -> Result<Self, String> {
        if inputs.is_empty() || inputs.len() > MAX_LLM_RESULT_INPUTS {
            return Err("result manifest must contain between 1 and 1024 inputs".to_string());
        }
        if declared_record_count == 0 || declared_record_count > MAX_LLM_RESULT_RECORDS {
            return Err("result manifest record count is outside its bound".to_string());
        }
        if declared_byte_size == 0 || declared_byte_size > MAX_LLM_RESULT_BYTES {
            return Err("result manifest byte size is outside its bound".to_string());
        }
        for pair in inputs.windows(2) {
            if pair[0].sequence >= pair[1].sequence {
                return Err("result manifest input sequences must be strictly ordered".to_string());
            }
        }
        let task = ResultTask::parse(task)?;
        let minimum_record_count = match status {
            ResultStatus::Completed => inputs
                .len()
                .checked_mul(2)
                .and_then(|count| {
                    count.checked_add(inputs.len() * usize::from(!task.permits_empty_input()))
                })
                .ok_or_else(|| "minimum result record count overflowed".to_string())?,
            ResultStatus::Failed => 1,
        };
        if usize::try_from(declared_record_count)
            .map_err(|_| "result record count exceeds this platform".to_string())?
            < minimum_record_count
        {
            return Err("result manifest contains too few records for its task".to_string());
        }
        Ok(Self {
            task,
            status,
            inputs: inputs.to_vec(),
            declared_record_count,
            next_record_sequence: 0,
            input_index: 0,
            input_state: InputState::AwaitingStart,
            continuation_count: 0,
            aggregate_normalized_bytes: 0,
            failed_record_seen: false,
        })
    }

    pub fn push(&mut self, record: ResultRecord<'_>) -> Result<DecodedResultPayload, String> {
        if self.next_record_sequence >= self.declared_record_count {
            return Err("result stream contains more records than declared".to_string());
        }
        if record.record_sequence != self.next_record_sequence {
            return Err("result record sequence is not contiguous".to_string());
        }
        if record.flags != 0 {
            return Err("result record flags are unsupported".to_string());
        }
        let decoded = decode_payload(record.kind, record.payload)?;
        match self.status {
            ResultStatus::Failed => self.push_failed(record, &decoded)?,
            ResultStatus::Completed => self.push_completed(record, &decoded)?,
        }
        self.next_record_sequence = self
            .next_record_sequence
            .checked_add(1)
            .ok_or_else(|| "result record sequence overflowed".to_string())?;
        Ok(decoded)
    }

    pub fn finish(self) -> Result<(), String> {
        if self.next_record_sequence != self.declared_record_count {
            return Err("result stream record count does not match its manifest".to_string());
        }
        match self.status {
            ResultStatus::Failed if self.failed_record_seen => Ok(()),
            ResultStatus::Failed => Err("failed result stream has no failure record".to_string()),
            ResultStatus::Completed
                if self.input_index == self.inputs.len()
                    && self.input_state == InputState::AwaitingStart =>
            {
                Ok(())
            }
            ResultStatus::Completed => {
                Err("completed result stream ended inside an input".to_string())
            }
        }
    }

    fn push_failed(
        &mut self,
        record: ResultRecord<'_>,
        decoded: &DecodedResultPayload,
    ) -> Result<(), String> {
        if self.failed_record_seen
            || record.kind != ResultRecordKind::Failure
            || record.input_sequence != u32::MAX
            || !matches!(decoded, DecodedResultPayload::Failure(_))
        {
            return Err(
                "failed result must contain exactly one unscoped failure record".to_string(),
            );
        }
        self.charge_normalized(decoded)?;
        self.failed_record_seen = true;
        Ok(())
    }

    fn push_completed(
        &mut self,
        record: ResultRecord<'_>,
        decoded: &DecodedResultPayload,
    ) -> Result<(), String> {
        let expected = self
            .inputs
            .get(self.input_index)
            .ok_or_else(|| "result stream contains records after its final input".to_string())?;
        if record.input_sequence != expected.sequence {
            return Err("result record input sequence does not match its manifest".to_string());
        }
        match self.input_state {
            InputState::AwaitingStart => {
                let DecodedResultPayload::InputStarted(started) = decoded else {
                    return Err("result input must begin with input-started".to_string());
                };
                if started.frame_timestamp_ms != expected.frame_timestamp_ms {
                    return Err("result input timestamp does not match its manifest".to_string());
                }
                self.input_state = if self.task.permits_empty_input() {
                    InputState::AcceptingValues
                } else {
                    InputState::AwaitingPrimary
                };
                self.continuation_count = 0;
            }
            InputState::AwaitingPrimary => {
                if record.kind != self.task.primary_kind() {
                    return Err("result input primary record does not match its task".to_string());
                }
                self.charge_normalized(decoded)?;
                self.input_state = InputState::AcceptingValues;
            }
            InputState::AcceptingValues => {
                if record.kind == ResultRecordKind::InputFinished {
                    self.input_index += 1;
                    self.input_state = InputState::AwaitingStart;
                    self.continuation_count = 0;
                    return Ok(());
                }
                if self.task == ResultTask::FaceDetection && record.kind == ResultRecordKind::Face {
                    self.charge_normalized(decoded)?;
                    return Ok(());
                }
                if self.task.continuation_kind() == Some(record.kind) {
                    self.continuation_count = self
                        .continuation_count
                        .checked_add(1)
                        .ok_or_else(|| "result continuation count overflowed".to_string())?;
                    if self.continuation_count > MAX_LLM_RESULT_CONTINUATIONS_PER_VALUE {
                        return Err("result value has too many continuation records".to_string());
                    }
                    self.charge_normalized(decoded)?;
                    return Ok(());
                }
                return Err("result record is not valid in the current input state".to_string());
            }
        }
        Ok(())
    }

    fn charge_normalized(&mut self, decoded: &DecodedResultPayload) -> Result<(), String> {
        let bytes = normalized_heap_bytes(decoded)?;
        self.aggregate_normalized_bytes = self
            .aggregate_normalized_bytes
            .checked_add(bytes)
            .ok_or_else(|| "result normalized aggregate overflowed".to_string())?;
        if self.aggregate_normalized_bytes > MAX_NORMALIZED_RESULT_RECORD_BYTES {
            return Err("result text/tag aggregate exceeds 2 MiB".to_string());
        }
        Ok(())
    }
}

impl ResultRecordCollector {
    pub fn new(
        task: &str,
        status: ResultStatus,
        inputs: &[ResultInputCorrelation],
        declared_record_count: u32,
        declared_byte_size: u64,
    ) -> Result<Self, String> {
        let task = ResultTask::parse(task)?;
        let validator = ResultRecordStreamValidator::new(
            task.as_str(),
            status,
            inputs,
            declared_record_count,
            declared_byte_size,
        )?;
        let mut collected_inputs = Vec::new();
        collected_inputs
            .try_reserve_exact(inputs.len())
            .map_err(|error| format!("could not reserve result inputs: {error}"))?;
        Ok(Self {
            validator,
            task,
            status,
            current: None,
            inputs: collected_inputs,
            failure: None,
        })
    }

    pub fn push(&mut self, record: ResultRecord<'_>) -> Result<(), String> {
        let input_sequence = record.input_sequence;
        let decoded = self.validator.push(record)?;
        match decoded {
            DecodedResultPayload::Failure(payload) => self.failure = Some(payload.error),
            DecodedResultPayload::InputStarted(payload) => {
                if self.current.is_some() {
                    return Err("result input started before the prior input finished".to_string());
                }
                self.current = Some(ResultInputAccumulator::new(
                    input_sequence,
                    payload.frame_timestamp_ms,
                ));
            }
            DecodedResultPayload::InputFinished => {
                let current = self
                    .current
                    .take()
                    .ok_or_else(|| "result input finished without a started input".to_string())?;
                self.inputs.push(current.finish(self.task)?);
            }
            payload => self
                .current
                .as_mut()
                .ok_or_else(|| "result value appeared outside an input".to_string())?
                .push(payload)?,
        }
        Ok(())
    }

    pub fn finish(self) -> Result<ValidatedResultStream, String> {
        self.validator.finish()?;
        if self.current.is_some() {
            return Err("result stream ended with an unfinished input".to_string());
        }
        match self.status {
            ResultStatus::Completed if self.failure.is_some() => {
                Err("completed result stream contains a failure".to_string())
            }
            ResultStatus::Failed if self.failure.is_none() => {
                Err("failed result stream contains no failure".to_string())
            }
            ResultStatus::Failed if !self.inputs.is_empty() => {
                Err("failed result stream contains completed inputs".to_string())
            }
            _ => Ok(ValidatedResultStream {
                status: self.status,
                inputs: self.inputs,
                failure: self.failure,
            }),
        }
    }
}

impl ResultTask {
    fn as_str(self) -> &'static str {
        match self {
            Self::Ocr => "ocr",
            Self::ImageTagging => "image_tagging",
            Self::ImageClustering => "image_clustering",
            Self::ImageAesthetics => "image_aesthetics",
            Self::FaceDetection => "face_detection",
            Self::ScreenshotDetection => "screenshot_detection",
            Self::DocumentDetection => "document_detection",
        }
    }
}

impl ResultInputAccumulator {
    fn new(sequence: u32, frame_timestamp_ms: Option<i64>) -> Self {
        Self {
            sequence,
            frame_timestamp_ms,
            text: String::new(),
            tags: Vec::new(),
            clustering: None,
            aesthetics: None,
            faces: Vec::new(),
            classification: None,
        }
    }

    fn push(&mut self, payload: DecodedResultPayload) -> Result<(), String> {
        match payload {
            DecodedResultPayload::OcrText(payload)
            | DecodedResultPayload::OcrTextContinuation(payload) => {
                self.text
                    .try_reserve_exact(payload.text.len())
                    .map_err(|error| format!("could not reserve OCR result: {error}"))?;
                self.text.push_str(&payload.text);
            }
            DecodedResultPayload::ImageTags(payload)
            | DecodedResultPayload::ImageTagsContinuation(payload) => {
                self.tags
                    .try_reserve_exact(payload.tags.len())
                    .map_err(|error| format!("could not reserve image tags: {error}"))?;
                self.tags.extend(payload.tags);
            }
            DecodedResultPayload::ImageClustering(payload) => self.clustering = Some(payload),
            DecodedResultPayload::ImageAesthetics(payload) => self.aesthetics = Some(payload),
            DecodedResultPayload::Face(payload) => {
                self.faces
                    .try_reserve_exact(1)
                    .map_err(|error| format!("could not reserve face result: {error}"))?;
                self.faces.push(payload);
            }
            DecodedResultPayload::ScreenshotDetection(payload)
            | DecodedResultPayload::DocumentDetection(payload) => {
                self.classification = Some(payload);
            }
            DecodedResultPayload::Failure(_)
            | DecodedResultPayload::InputStarted(_)
            | DecodedResultPayload::InputFinished => {
                return Err("result payload is invalid inside an input".to_string());
            }
        }
        Ok(())
    }

    fn finish(self, task: ResultTask) -> Result<ValidatedResultInput, String> {
        let value = match task {
            ResultTask::Ocr => ValidatedResultValue::Ocr(self.text),
            ResultTask::ImageTagging => ValidatedResultValue::ImageTags(self.tags),
            ResultTask::ImageClustering => ValidatedResultValue::ImageClustering(
                self.clustering
                    .ok_or_else(|| "clustering result is missing".to_string())?,
            ),
            ResultTask::ImageAesthetics => ValidatedResultValue::ImageAesthetics(
                self.aesthetics
                    .ok_or_else(|| "aesthetics result is missing".to_string())?,
            ),
            ResultTask::FaceDetection => ValidatedResultValue::Faces(self.faces),
            ResultTask::ScreenshotDetection => ValidatedResultValue::ScreenshotDetection(
                self.classification
                    .ok_or_else(|| "screenshot result is missing".to_string())?,
            ),
            ResultTask::DocumentDetection => ValidatedResultValue::DocumentDetection(
                self.classification
                    .ok_or_else(|| "document result is missing".to_string())?,
            ),
        };
        Ok(ValidatedResultInput {
            sequence: self.sequence,
            frame_timestamp_ms: self.frame_timestamp_ms,
            value,
        })
    }
}
