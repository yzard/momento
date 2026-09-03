use serde::Serialize;
use serde_json::Value;

use crate::models::*;

#[derive(Debug)]
pub struct ParsedControlRequest {
    pub request: Result<ControlRequest, ControlRequestParseError>,
    pub log_payload: String,
}

#[derive(Debug)]
pub struct ControlRequestParseError {
    pub detail: String,
    pub data_error: bool,
}

pub(crate) fn parse_control_request(
    kind: ControlRequestKind,
    bytes: &[u8],
) -> ParsedControlRequest {
    let value = match serde_json::from_slice::<Value>(bytes) {
        Ok(value) => value,
        Err(error) => {
            return ParsedControlRequest {
                request: Err(ControlRequestParseError {
                    detail: error.to_string(),
                    data_error: false,
                }),
                log_payload: crate::logging::render_invalid_control_payload(bytes),
            };
        }
    };
    let request =
        ControlRequest::parse(kind, value.clone()).map_err(|error| ControlRequestParseError {
            detail: error.to_string(),
            data_error: true,
        });
    let mut redacted = value;
    crate::logging::redact_request_values(&mut redacted);
    ParsedControlRequest {
        request,
        log_payload: redacted.to_string(),
    }
}

macro_rules! control_request_types {
    ($($variant:ident => $request:ty),+ $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum ControlRequestKind {
            $($variant),+
        }

        #[derive(Debug)]
        pub enum ControlRequest {
            $($variant($request)),+
        }

        impl ControlRequest {
            pub(crate) fn parse(
                kind: ControlRequestKind,
                value: Value,
            ) -> Result<Self, serde_json::Error> {
                match kind {
                    $(ControlRequestKind::$variant => {
                        serde_json::from_value::<$request>(value).map(Self::$variant)
                    }),+
                }
            }
        }

        pub trait ControlRequestDto: Sized + Send + 'static {
            const KIND: ControlRequestKind;
            fn from_control_request(request: ControlRequest) -> Option<Self>;
        }

        $(impl ControlRequestDto for $request {
            const KIND: ControlRequestKind = ControlRequestKind::$variant;

            fn from_control_request(request: ControlRequest) -> Option<Self> {
                match request {
                    ControlRequest::$variant(request) => Some(request),
                    _ => None,
                }
            }
        })+
    };
}

control_request_types! {
    AiScheduleUpdate => AiScheduleUpdateRequest,
    UserCreate => UserCreateRequest,
    UserUpdate => UserUpdateRequest,
    UserDelete => UserDeleteRequest,
    FaceGroupsList => FaceGroupsListRequest,
    FaceGroup => FaceGroupRequest,
    FaceGroupsMerge => FaceGroupsMergeRequest,
    MapClusters => MapClustersRequest,
    MapMedia => MapMediaRequest,
    ShareVerify => ShareVerifyRequest,
    BackupDeviceRegister => BackupDeviceRegisterRequest,
    BackupUploadCreate => BackupUploadCreateRequest,
    BackupUploadId => BackupUploadIdRequest,
    AlbumCreate => AlbumCreateRequest,
    AlbumUpdate => AlbumUpdateRequest,
    AlbumDelete => AlbumDeleteRequest,
    AlbumAddMedia => AlbumAddMediaRequest,
    AlbumRemoveMedia => AlbumRemoveMediaRequest,
    AlbumGet => AlbumGetRequest,
    AlbumReorder => AlbumReorderRequest,
    Metadata => MetadataRequest,
    TrashRestore => TrashRestoreRequest,
    TrashDelete => TrashDeleteRequest,
    FileOperationList => FileOperationListRequest,
    FileOperationGet => FileOperationGetRequest,
    FileOperationRetry => FileOperationRetryRequest,
    DeduplicateGroups => DeduplicateGroupsRequest,
    TimelineList => TimelineListRequest,
    TimelineMarkers => TimelineMarkersRequest,
    MediaBatch => MediaBatchRequest,
    MediaUpdate => MediaUpdateRequest,
    MediaDelete => MediaDeleteRequest,
    MediaAccessTicket => MediaAccessTicketRequest,
    RefreshToken => RefreshTokenRequest,
    Logout => LogoutRequest,
    ChangePassword => ChangePasswordRequest,
    PlacesList => PlacesListRequest,
    PlaceGet => PlaceGetRequest,
    ShareCreate => ShareCreateRequest,
    ShareDelete => ShareDeleteRequest,
    ShareMedia => ShareMediaRequest,
    ShareAlbum => ShareAlbumRequest,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageResponse {
    pub message: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct ErrorResponse {
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<&'static str>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FaceGroupMergeResponse {
    pub group: FaceGroupResponse,
}

#[derive(Debug, Serialize)]
pub struct PublicMediaContentResponse {
    #[serde(rename = "type")]
    pub content_type: String,
    pub media: MediaResponse,
}

#[derive(Debug, Serialize)]
pub struct PublicAlbumSummaryResponse {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PublicAlbumContentResponse {
    #[serde(rename = "type")]
    pub content_type: String,
    pub album: PublicAlbumSummaryResponse,
    pub media: Vec<MediaResponse>,
}

#[derive(Debug, Serialize)]
pub struct HealthcheckResponse {
    pub status: String,
    pub version: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilitiesResponse {
    pub app_version: String,
    pub api_version: u8,
    pub supported_media_extensions: Vec<String>,
    pub features: FeatureFlagsResponse,
    pub backup: BackupCapabilitiesResponse,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeatureFlagsResponse {
    pub llm: bool,
    pub image_tagging: bool,
    pub deduplicate: bool,
    pub face_detection: bool,
    pub image_aesthetics: bool,
    pub screenshot_detection: bool,
    pub document_detection: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupCapabilitiesResponse {
    pub enabled: bool,
    pub protocol_version: u8,
    pub max_upload_bytes: u64,
    pub max_chunk_bytes: u64,
    pub session_expiry_hours: u64,
}

macro_rules! control_response_types {
    ($($variant:ident => $response:ty),+ $(,)?) => {
        pub enum ControlResponse {
            $($variant($response)),+
        }

        $(impl From<$response> for ControlResponse {
            fn from(response: $response) -> Self {
                Self::$variant(response)
            }
        })+

        impl Serialize for ControlResponse {
            fn serialize<Serializer>(
                &self,
                serializer: Serializer,
            ) -> Result<Serializer::Ok, Serializer::Error>
            where
                Serializer: serde::Serializer,
            {
                match self {
                    $(Self::$variant(response) => response.serialize(serializer)),+
                }
            }
        }
    };
}

control_response_types! {
    Error => ErrorResponse,
    AiAction => AiActionResponse,
    AiStatus => AiStatusResponse,
    AiFeatureSchedule => AiFeatureScheduleResponse,
    User => UserResponse,
    UserList => UserListResponse,
    FaceGroupsList => FaceGroupsListResponse,
    FaceGroupMedia => FaceGroupMediaResponse,
    FaceGroupMerge => FaceGroupMergeResponse,
    AlbumDetail => AlbumDetailResponse,
    Album => AlbumResponse,
    AlbumList => AlbumListResponse,
    FileOperationList => FileOperationListResponse,
    FileOperationDetail => FileOperationDetailResponse,
    FileOperationRetry => FileOperationRetryResponse,
    DeduplicateGroups => DeduplicateGroupsResponse,
    Token => TokenResponse,
    ShareLink => ShareLinkResponse,
    ShareList => ShareListResponse,
    ShareVerify => ShareVerifyResponse,
    ImportTrigger => ImportTriggerResponse,
    ImportStatus => ImportStatusResponse,
    MetadataAction => MetadataActionResponse,
    MetadataStatus => MetadataStatusResponse,
    TrashList => TrashListResponse,
    Trash => TrashResponse,
    TimelineList => TimelineListResponse,
    TimelineMarkers => TimelineMarkersResponse,
    MediaBatch => MediaBatchResponse,
    Media => MediaResponse,
    DeleteMedia => DeleteMediaResponse,
    MediaAccessTicket => MediaAccessTicketResponse,
    PlacesList => PlacesListResponse,
    PlaceGet => PlaceGetResponse,
    MapClusters => MapClustersResponse,
    MapMediaList => MapMediaListResponse,
    BackupDeviceRegister => BackupDeviceRegisterResponse,
    BackupUpload => BackupUploadResponse,
    Message => MessageResponse,
    PublicMediaContent => PublicMediaContentResponse,
    PublicAlbumContent => PublicAlbumContentResponse,
    Healthcheck => HealthcheckResponse,
    Capabilities => CapabilitiesResponse,
}
