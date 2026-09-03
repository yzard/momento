#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(usize)]
pub enum DurableSourceId {
    MediaProcess,
    LocalImport,
    WebDavImport,
    BackupImport,
    Metadata,
    LlmSubmission,
    LlmCancellation,
    LlmResult,
    DeduplicateFinalization,
    FaceGroupFinalization,
    JournalRecovery,
    FileCleanup,
    Maintenance,
}

impl DurableSourceId {
    pub const ALL: [Self; 13] = [
        Self::MediaProcess,
        Self::LocalImport,
        Self::WebDavImport,
        Self::BackupImport,
        Self::Metadata,
        Self::LlmSubmission,
        Self::LlmCancellation,
        Self::LlmResult,
        Self::DeduplicateFinalization,
        Self::FaceGroupFinalization,
        Self::JournalRecovery,
        Self::FileCleanup,
        Self::Maintenance,
    ];
    pub const COUNT: usize = Self::ALL.len();

    pub(crate) const fn index(self) -> usize {
        self as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CronTaskId {
    Ocr,
    ImageTagging,
    Deduplicate,
    ImageAesthetics,
    FaceDetection,
    ScreenshotDetection,
    DocumentDetection,
}

impl CronTaskId {
    pub const ALL: [Self; 7] = [
        Self::Ocr,
        Self::ImageTagging,
        Self::Deduplicate,
        Self::ImageAesthetics,
        Self::FaceDetection,
        Self::ScreenshotDetection,
        Self::DocumentDetection,
    ];
    pub const COUNT: usize = Self::ALL.len();
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub enum SchedulerAdmissionKind {
    NewClaim,
    ExistingClaimCompletion,
    RecoveryHandoff,
}

impl SchedulerAdmissionKind {
    pub const ALL: [Self; 3] = [
        Self::NewClaim,
        Self::ExistingClaimCompletion,
        Self::RecoveryHandoff,
    ];
    pub const COUNT: usize = Self::ALL.len();

    pub(crate) const fn index(self) -> usize {
        self as usize
    }
}
