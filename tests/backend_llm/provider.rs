use futures::stream::{FuturesUnordered, StreamExt};
use llm_service::config::{Config, GeneralConfig, ServiceConfig, StorageConfig};
use llm_service::provider::{InferenceInput, ServiceManager};
use sha2::{Digest, Sha256};
use std::fs;
use std::net::TcpListener;
use std::path::Path;
use std::sync::Arc;
use tempfile::{NamedTempFile, TempDir};

const MOCK_RUNTIME: &str = r#"
import argparse
import base64
import json
import os
import struct
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

parser = argparse.ArgumentParser()
parser.add_argument('--mode', required=True)
parser.add_argument('--port', required=True, type=int)
parser.add_argument('--start-log', required=True)
parser.add_argument('--max-concurrent-jobs', required=True, type=int)
parser.add_argument('--input-root', required=True)
parser.add_argument('--mount-source', required=True)
parser.add_argument('--minimum-face-likelihood', type=float)
parser.add_argument('--minimum-face-resolution-pixels', type=int)
arguments = parser.parse_args()
if 'face_detection' in arguments.mode:
    if arguments.minimum_face_likelihood != 0.8:
        raise RuntimeError('invalid minimum face likelihood')
    if arguments.minimum_face_resolution_pixels != 112:
        raise RuntimeError('invalid minimum face resolution')

with open(arguments.start_log, 'a', encoding='utf-8') as output:
    output.write(arguments.mode + ':' + str(arguments.max_concurrent_jobs) + '\n')

active_requests = 0
maximum_active_requests = 0
request_lock = threading.Lock()

class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path == '/ready':
            self.send_json(200, {'status': 'ready'})
            return
        if arguments.mode == 'ocr' and self.path == '/v1/models':
            self.send_json(200, {'data': []})
            return
        if self.path == '/metrics':
            self.send_json(200, {'maximumActiveRequests': maximum_active_requests})
            return
        self.send_error(404)

    def do_POST(self):
        if arguments.mode == 'ocr' and self.path == '/v1/chat/completions':
            content_length = int(self.headers.get('Content-Length', '0'))
            request = json.loads(self.rfile.read(content_length))
            image_url = request['messages'][0]['content'][1]['image_url']['url']
            if not image_url.startswith('file://' + arguments.input_root + '/'):
                self.send_json(400, {'detail': 'OCR input was not a local file URL'})
                return
            self.send_json(200, {'choices': [{'message': {'content': 'OCR text'}}]})
            return
        if self.path != '/infer':
            self.send_error(404)
            return
        content_length = int(self.headers.get('Content-Length', '0'))
        request_body = self.rfile.read(content_length)
        if arguments.mode == 'crash_face_detection':
            os._exit(1)
        try:
            descriptor = json.loads(request_body)
        except (UnicodeDecodeError, json.JSONDecodeError):
            self.send_json(400, {'detail': 'input was not a JSON descriptor'})
            return
        if set(descriptor) != {'jobId', 'sequence', 'byteSize', 'contentHash', 'mimeType'}:
            self.send_json(400, {'detail': 'invalid input descriptor'})
            return
        if arguments.mode == 'image_tagging':
            self.send_json(200, {'tags': ['person', 'bicycle']})
            return
        if arguments.mode in ('face_detection', 'slow_face_detection', 'rolling_face_detection'):
            global active_requests, maximum_active_requests
            with request_lock:
                active_requests += 1
                maximum_active_requests = max(maximum_active_requests, active_requests)
            if arguments.mode == 'slow_face_detection':
                time.sleep(0.25)
            if arguments.mode == 'rolling_face_detection':
                with open(arguments.start_log, 'a', encoding='utf-8') as output:
                    output.write('start:' + descriptor['jobId'] + '\n')
                time.sleep(0.25 if descriptor['jobId'].endswith('0') else 0.02)
            embedding = [1.0] + [0.0] * 511
            encoded = base64.b64encode(struct.pack('<512f', *embedding)).decode('ascii')
            self.send_json(200, {'faces': [{
                'index': 0,
                'boundingBox': {'x': 0.1, 'y': 0.2, 'width': 0.3, 'height': 0.4},
                'eyeCenter': {'x': 0.25, 'y': 0.32},
                'confidence': 0.95,
                'qualityScore': 0.8,
                'frontalityScore': 0.9,
                'embedding': encoded,
                'embeddingEncoding': 'float32_le',
                'embeddingDimensions': 512,
            }]})
            with request_lock:
                active_requests -= 1
            if arguments.mode == 'rolling_face_detection':
                with open(arguments.start_log, 'a', encoding='utf-8') as output:
                    output.write('finish:' + descriptor['jobId'] + '\n')
            return
        if arguments.mode == 'malformed_face_detection':
            self.send_json(200, {'faces': [{
                'index': 0,
                'boundingBox': {'x': 0.1, 'y': 0.2, 'width': 0.3, 'height': 0.4},
                'eyeCenter': {'x': 1.5, 'y': 0.32},
                'confidence': 0.95,
                'qualityScore': 0.8,
                'frontalityScore': 0.9,
                'embedding': '',
                'embeddingEncoding': 'float32_le',
                'embeddingDimensions': 512,
            }]})
            return
        embedding = [1.0] + [0.0] * 383
        encoded = base64.b64encode(struct.pack('<384f', *embedding)).decode('ascii')
        if arguments.mode == 'malformed_clustering':
            encoded = base64.b64encode(struct.pack('<f', 1.0)).decode('ascii')
        self.send_json(200, {
            'embedding': encoded,
            'embeddingEncoding': 'float32_le',
            'embeddingDimensions': 384,
            'perceptualHash': '0123456789abcdef',
            'qualityScore': 0.75,
        })

    def log_message(self, message_format, *args):
        return

    def send_json(self, status, payload):
        body = json.dumps(payload).encode('utf-8')
        self.send_response(status)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)

ThreadingHTTPServer(('127.0.0.1', arguments.port), Handler).serve_forever()
"#;

fn available_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to reserve test port");
    listener
        .local_addr()
        .expect("Failed to read test port")
        .port()
}

#[test]
fn runtime_log_redacts_base64_payloads() {
    let encoded = "a".repeat(64);
    let line = format!(r#"face={{"embedding":"{encoded}","embeddingDimensions":512}} complete"#);

    let redacted = llm_service::provider::redact_base64_text(&line);

    assert!(!redacted.contains(&encoded));
    assert!(redacted.contains("[base64 omitted]"));
    assert!(redacted.contains("embeddingDimensions"));
}

pub(super) fn service(
    model_type: &str,
    mode: &str,
    port: u16,
    script_path: &Path,
    start_log: &Path,
) -> ServiceConfig {
    let mut docker_command = vec![
        "python3".to_string(),
        "{script_path}".to_string(),
        "--mode".to_string(),
        mode.to_string(),
        "--port".to_string(),
        port.to_string(),
        "--start-log".to_string(),
        start_log.to_string_lossy().into_owned(),
        "--max-concurrent-jobs".to_string(),
        "{max_concurrent_jobs}".to_string(),
        "--input-root".to_string(),
        "{runtime_mount_target}".to_string(),
        "--mount-source".to_string(),
        "{runtime_mount_source}".to_string(),
    ];
    if model_type == "face_detection" {
        docker_command.extend([
            "--minimum-face-likelihood".to_string(),
            "{minimum_face_likelihood}".to_string(),
            "--minimum-face-resolution-pixels".to_string(),
            "{minimum_face_resolution_pixels}".to_string(),
        ]);
    }
    ServiceConfig {
        enabled: true,
        model_type: model_type.to_string(),
        model_version: if model_type == "ocr" {
            "unlimited_ocr".to_string()
        } else if model_type == "image_clustering" {
            "dinov2-small".to_string()
        } else if model_type == "face_detection" {
            "buffalo_l".to_string()
        } else {
            "ram++".to_string()
        },
        docker_command,
        device: "cpu".to_string(),
        base_url: if model_type == "ocr" {
            format!("http://127.0.0.1:{port}/v1")
        } else {
            format!("http://127.0.0.1:{port}")
        },
        model: if model_type == "ocr" {
            "baidu/Unlimited-OCR".to_string()
        } else if model_type == "image_clustering" {
            "facebook/dinov2-small".to_string()
        } else if model_type == "face_detection" {
            "buffalo_l".to_string()
        } else {
            String::new()
        },
        script_path: script_path.to_string_lossy().into_owned(),
        startup_timeout_seconds: 5,
        request_timeout_seconds: 5,
        max_tokens: 0,
        embedding_dimensions: if model_type == "image_clustering" {
            384
        } else if model_type == "face_detection" {
            512
        } else {
            0
        },
        minimum_face_likelihood: (model_type == "face_detection").then_some(0.8),
        minimum_face_resolution_pixels: (model_type == "face_detection").then_some(112),
        max_concurrent_jobs: 2,
    }
}

pub(super) fn manager(services: Vec<ServiceConfig>) -> ServiceManager {
    let fixture_root = Path::new(&services[0].script_path)
        .parent()
        .expect("fixture script parent");
    let queue_dir = fixture_root.join("queue");
    fs::create_dir_all(queue_dir.join("processing")).expect("test processing queue");
    ServiceManager::new(Arc::new(Config {
        general: GeneralConfig::default(),
        storage: StorageConfig {
            data_dir: fixture_root.to_path_buf(),
            queue_dir: queue_dir.clone(),
            runtime_mount_source: Path::new("").to_path_buf(),
            runtime_mount_target: queue_dir.join("processing"),
        },
        callback: Default::default(),
        service: services,
    }))
}

pub(super) fn fixture() -> (TempDir, std::path::PathBuf, std::path::PathBuf) {
    let directory = TempDir::new().expect("Failed to create runtime fixture");
    let script_path = directory.path().join("mock_runtime.py");
    let start_log = directory.path().join("starts.log");
    fs::write(&script_path, MOCK_RUNTIME).expect("Failed to write mock runtime");
    fs::write(
        directory.path().join("runtime_input.py"),
        include_str!("../../src/backend_llm/runtime_input.py"),
    )
    .expect("Failed to write runtime input helper");
    (directory, script_path, start_log)
}

async fn infer_one(
    manager: &mut ServiceManager,
    task: &str,
    bytes: &[u8],
    filename: &str,
) -> Result<llm_service::provider::InferenceResponse, llm_service::error::ServiceError> {
    let input_file = NamedTempFile::new().expect("Failed to create queued input");
    fs::write(input_file.path(), bytes).expect("Failed to write queued input");
    let dispatcher = manager.dispatcher(task).await?;
    let inputs = dispatcher
        .infer_inputs(vec![InferenceInput {
            job_id: "abcdef12".to_string(),
            sequence: 0,
            frame_timestamp_ms: None,
            path: input_file.path().to_path_buf(),
            byte_size: bytes.len() as u64,
            content_hash: format!("{:x}", Sha256::digest(bytes)),
            mime_type: "image/jpeg".to_string(),
            filename: filename.to_string(),
        }])
        .await;
    Ok(inputs?
        .into_iter()
        .next()
        .expect("single input response")
        .response)
}

#[tokio::test]
async fn manager_reuses_a_runtime_and_switches_for_a_different_task() {
    let (_directory, script_path, start_log) = fixture();
    let clustering_port = available_port();
    let tagging_port = available_port();
    let clustering = service(
        "image_clustering",
        "image_clustering",
        clustering_port,
        &script_path,
        &start_log,
    );
    let tagging = service(
        "image_tagging",
        "image_tagging",
        tagging_port,
        &script_path,
        &start_log,
    );
    let mut manager = manager(vec![clustering, tagging]);

    let first = infer_one(
        &mut manager,
        "image_clustering",
        b"first image",
        "first.jpg",
    )
    .await
    .expect("First clustering request should succeed");
    let second = infer_one(
        &mut manager,
        "image_clustering",
        b"second image",
        "second.jpg",
    )
    .await
    .expect("Second clustering request should reuse the runtime");

    assert_eq!(first.embedding_dimensions, Some(384));
    assert_eq!(first.embedding_encoding.as_deref(), Some("float32_le"));
    assert!(first
        .embedding
        .as_ref()
        .is_some_and(|embedding| !embedding.is_empty()));
    assert_eq!(first.quality_score, Some(0.75));
    let serialized = serde_json::to_value(&first).expect("Response should serialize");
    assert_eq!(serialized["embeddingEncoding"], "float32_le");
    assert_eq!(serialized["embeddingDimensions"], 384);
    assert_eq!(serialized["perceptualHash"], "0123456789abcdef");
    assert_eq!(serialized["qualityScore"], 0.75);
    assert_eq!(second.perceptual_hash.as_deref(), Some("0123456789abcdef"));
    assert_eq!(manager.active_name(), "dinov2");
    assert_eq!(
        fs::read_to_string(&start_log).expect("Failed to read runtime starts"),
        "image_clustering:2\n"
    );

    let tagging_response = infer_one(&mut manager, "image_tagging", b"tag image", "tag.jpg")
        .await
        .expect("Tagging should switch from clustering");

    assert_eq!(tagging_response.tags, vec!["person", "bicycle"]);
    assert_eq!(manager.active_name(), "ram++");
    assert_eq!(
        fs::read_to_string(&start_log).expect("Failed to read runtime starts"),
        "image_clustering:2\nimage_tagging:2\n"
    );

    manager
        .shutdown()
        .await
        .expect("Runtime should stop cleanly");
    assert_eq!(manager.active_name(), "on-demand");
}

#[tokio::test]
async fn manager_sends_ocr_a_local_queue_file_url() {
    let (_directory, script_path, start_log) = fixture();
    let port = available_port();
    let ocr = service("ocr", "ocr", port, &script_path, &start_log);
    let mut manager = manager(vec![ocr]);

    let response = infer_one(&mut manager, "ocr", b"ocr image", "ocr.jpg")
        .await
        .expect("OCR request should succeed");

    assert_eq!(response.text, "OCR text");
    manager.shutdown().await.expect("runtime shutdown");
}

#[tokio::test]
async fn manager_preserves_ordered_frame_input_correlation() {
    let (directory, script_path, start_log) = fixture();
    let port = available_port();
    let clustering = service(
        "image_clustering",
        "image_clustering",
        port,
        &script_path,
        &start_log,
    );
    let mut manager = manager(vec![clustering]);
    let first_path = directory.path().join("first-frame.jpg");
    let second_path = directory.path().join("second-frame.jpg");
    fs::write(&first_path, b"first frame").expect("first frame");
    fs::write(&second_path, b"second frame").expect("second frame");
    let dispatcher = manager
        .dispatcher("image_clustering")
        .await
        .expect("clustering dispatcher");
    let first_job = dispatcher
        .infer_inputs(vec![
            InferenceInput {
                job_id: "abcdef12".to_string(),
                sequence: 0,
                frame_timestamp_ms: Some(0),
                path: first_path,
                byte_size: 11,
                content_hash: format!("{:x}", Sha256::digest(b"first frame")),
                mime_type: "image/jpeg".to_string(),
                filename: "first.jpg".to_string(),
            },
            InferenceInput {
                job_id: "abcdef12".to_string(),
                sequence: 1,
                frame_timestamp_ms: Some(1000),
                path: second_path,
                byte_size: 12,
                content_hash: format!("{:x}", Sha256::digest(b"second frame")),
                mime_type: "image/jpeg".to_string(),
                filename: "second.jpg".to_string(),
            },
        ])
        .await
        .expect("first result");
    assert_eq!(first_job.len(), 2);
    assert_eq!(first_job[0].sequence, 0);
    assert_eq!(first_job[0].frame_timestamp_ms, Some(0));
    assert_eq!(first_job[1].sequence, 1);
    assert_eq!(first_job[1].frame_timestamp_ms, Some(1000));
    assert_eq!(
        fs::read_to_string(&start_log).expect("runtime starts"),
        "image_clustering:2\n"
    );
    manager.shutdown().await.expect("runtime shutdown");
}

#[tokio::test]
async fn manager_rejects_invalid_clustering_embedding_length() {
    let (_directory, script_path, start_log) = fixture();
    let port = available_port();
    let clustering = service(
        "image_clustering",
        "malformed_clustering",
        port,
        &script_path,
        &start_log,
    );
    let mut manager = manager(vec![clustering]);

    let error = infer_one(&mut manager, "image_clustering", b"image", "image.jpg")
        .await
        .expect_err("Short embedding should be rejected");

    assert!(error.to_string().contains("expected 1536"));
    manager
        .shutdown()
        .await
        .expect("Runtime should stop cleanly");
}

#[tokio::test]
async fn manager_reuses_face_runtime_and_serializes_ordered_faces() {
    let (_directory, script_path, start_log) = fixture();
    let face_port = available_port();
    let clustering_port = available_port();
    let face_detection = service(
        "face_detection",
        "face_detection",
        face_port,
        &script_path,
        &start_log,
    );
    let clustering = service(
        "image_clustering",
        "image_clustering",
        clustering_port,
        &script_path,
        &start_log,
    );
    let mut manager = manager(vec![face_detection, clustering]);

    let first = infer_one(&mut manager, "face_detection", b"image", "image.jpg")
        .await
        .expect("Face detection should succeed");
    let second = infer_one(&mut manager, "face_detection", b"image", "image.jpg")
        .await
        .expect("Face detection should reuse its runtime");

    assert_eq!(first.faces.len(), 1);
    assert_eq!(first.faces[0].index, 0);
    assert_eq!(first.faces[0].embedding_dimensions, 512);
    assert_eq!(first.faces[0].eye_center.x, 0.25);
    assert_eq!(first.faces[0].eye_center.y, 0.32);
    assert_eq!(first.faces[0].frontality_score, 0.9);
    assert_eq!(first.faces[0].embedding_encoding, "float32_le");
    assert_eq!(second.faces[0].bounding_box.width, 0.3);
    assert_eq!(manager.active_name(), "insightface");
    let serialized = serde_json::to_value(&first).expect("Face response should serialize");
    assert!(
        (serialized["faces"][0]["boundingBox"]["x"]
            .as_f64()
            .expect("serialized bounding box x")
            - 0.1)
            .abs()
            < 0.000_001
    );
    assert_eq!(serialized["faces"][0]["embeddingDimensions"], 512);
    assert_eq!(
        fs::read_to_string(&start_log).expect("runtime starts"),
        "face_detection:2\n"
    );

    infer_one(&mut manager, "image_clustering", b"image", "image.jpg")
        .await
        .expect("Clustering should switch from face detection");
    assert_eq!(manager.active_name(), "dinov2");
    assert_eq!(
        fs::read_to_string(&start_log).expect("runtime starts"),
        "face_detection:2\nimage_clustering:2\n"
    );
    manager.shutdown().await.expect("runtime shutdown");
}

#[tokio::test]
async fn manager_dispatches_all_face_jobs_without_a_provider_concurrency_limit() {
    let (directory, script_path, start_log) = fixture();
    let port = available_port();
    let face_detection = service(
        "face_detection",
        "slow_face_detection",
        port,
        &script_path,
        &start_log,
    );
    let mut manager = manager(vec![face_detection]);
    let inputs = (0..8)
        .map(|sequence| {
            let path = directory.path().join(format!("image-{sequence}.jpg"));
            fs::write(&path, b"image").expect("queued face input");
            InferenceInput {
                job_id: format!("abcdef{sequence:02x}"),
                sequence,
                frame_timestamp_ms: None,
                path,
                byte_size: 5,
                content_hash: format!("{:x}", Sha256::digest(b"image")),
                mime_type: "image/jpeg".to_string(),
                filename: format!("image-{sequence}.jpg"),
            }
        })
        .collect::<Vec<_>>();

    let dispatcher = manager
        .dispatcher("face_detection")
        .await
        .expect("face dispatcher");
    let mut in_flight = inputs
        .into_iter()
        .map(|input| dispatcher.infer_inputs(vec![input]))
        .collect::<FuturesUnordered<_>>();
    let mut results = Vec::new();
    while let Some(result) = in_flight.next().await {
        results.push(result);
    }
    let metrics: serde_json::Value = reqwest::get(format!("http://127.0.0.1:{port}/metrics"))
        .await
        .expect("metrics response")
        .json()
        .await
        .expect("metrics body");

    assert_eq!(results.len(), 8);
    assert!(results.iter().all(Result::is_ok));
    assert_eq!(metrics["maximumActiveRequests"], 8);
    manager.shutdown().await.expect("runtime shutdown");
}

#[tokio::test]
async fn face_runtime_connection_loss_is_retryable_by_the_scheduler() {
    let (_directory, script_path, start_log) = fixture();
    let port = available_port();
    let face_detection = service(
        "face_detection",
        "crash_face_detection",
        port,
        &script_path,
        &start_log,
    );
    let mut manager = manager(vec![face_detection]);

    let error = infer_one(&mut manager, "face_detection", b"image", "face.jpg")
        .await
        .expect_err("crashed runtime must fail the request");

    assert!(matches!(
        error,
        llm_service::error::ServiceError::RuntimeUnavailable(_)
    ));
    manager.shutdown().await.expect("runtime shutdown");
}

#[tokio::test]
async fn manager_rejects_invalid_face_eye_center() {
    let (_directory, script_path, start_log) = fixture();
    let port = available_port();
    let face_detection = service(
        "face_detection",
        "malformed_face_detection",
        port,
        &script_path,
        &start_log,
    );
    let mut manager = manager(vec![face_detection]);

    let error = infer_one(&mut manager, "face_detection", b"image", "image.jpg")
        .await
        .expect_err("Invalid eye center must be rejected");

    assert!(error.to_string().contains("normalized eye center"));
    manager.shutdown().await.expect("runtime shutdown");
}
