use futures::stream::{FuturesUnordered, StreamExt};
use llm_service::config::{Config, ServerConfig, ServiceConfig};
use llm_service::provider::{
    InferenceInput, RuntimeCatalog, RuntimeSpec, ServiceManager, ServiceType,
};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs;
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};
use tempfile::{NamedTempFile, TempDir};

const MOCK_RUNTIME: &str = r#"
import argparse
import base64
import json
import os
import subprocess
import struct
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

parser = argparse.ArgumentParser()
parser.add_argument('--mode', required=True)
parser.add_argument('--port', required=True, type=int)
parser.add_argument('--start-log', required=True)
parser.add_argument('--max-concurrent-jobs', type=int)
parser.add_argument('--processing-concurrency', type=int)
parser.add_argument('--model-concurrency', type=int)
parser.add_argument('--model-batch-wait-milliseconds', type=int)
parser.add_argument('--input-root', required=True)
parser.add_argument('--mount-source', required=True)
parser.add_argument('--cache-dir', required=True)
parser.add_argument('--minimum-face-likelihood', type=float)
parser.add_argument('--minimum-face-resolution-pixels', type=int)
parser.add_argument('--face-detection-size', type=int)
parser.add_argument('--recognition-batch-size', type=int)
parser.add_argument('--recognition-batch-wait-milliseconds', type=int)
arguments = parser.parse_args()
expected_cache_dir = os.path.join(os.path.dirname(arguments.start_log), 'llm', 'cache')
if arguments.cache_dir != expected_cache_dir:
    raise RuntimeError('invalid runtime cache directory')
if os.environ.get('XDG_CACHE_HOME') != expected_cache_dir:
    raise RuntimeError('invalid runtime cache environment')
if os.environ.get('HOME') != expected_cache_dir:
    raise RuntimeError('invalid runtime home directory')
if 'face_detection' in arguments.mode:
    if arguments.processing_concurrency != 2:
        raise RuntimeError('invalid processing concurrency')
    if arguments.model_concurrency != 2:
        raise RuntimeError('invalid model concurrency')
    if arguments.face_detection_size != 960:
        raise RuntimeError('invalid face detection size')
    if arguments.recognition_batch_size != 64:
        raise RuntimeError('invalid recognition batch size')
    if arguments.recognition_batch_wait_milliseconds != 5:
        raise RuntimeError('invalid recognition batch wait')
    if arguments.minimum_face_likelihood != 0.8:
        raise RuntimeError('invalid minimum face likelihood')
    if arguments.minimum_face_resolution_pixels != 112:
        raise RuntimeError('invalid minimum face resolution')
if 'image_aesthetics' in arguments.mode or 'image_clustering' in arguments.mode:
    if arguments.processing_concurrency != 2:
        raise RuntimeError('invalid dynamic batch processing concurrency')
    if arguments.model_concurrency != 2:
        raise RuntimeError('invalid dynamic batch model concurrency')
    if arguments.model_batch_wait_milliseconds != 5:
        raise RuntimeError('invalid dynamic batch model wait')

runtime_concurrency = arguments.model_concurrency or arguments.max_concurrent_jobs
with open(arguments.start_log, 'a', encoding='utf-8') as output:
    output.write(arguments.mode + ':' + str(runtime_concurrency) + '\n')

def start_worker(duration_seconds):
    worker_ready = arguments.start_log + '.worker-ready'
    worker = subprocess.Popen([
        'python3',
        '-c',
        "import signal, sys, time; signal.signal(signal.SIGTERM, signal.SIG_IGN); open(sys.argv[1], 'w').close(); time.sleep(float(sys.argv[2]))",
        worker_ready,
        str(duration_seconds),
    ])
    while not os.path.exists(worker_ready):
        time.sleep(0.01)
    with open(arguments.start_log, 'a', encoding='utf-8') as output:
        output.write('worker:' + str(worker.pid) + '\n')
    return worker

if arguments.mode == 'process_tree_image_tagging':
    worker = start_worker(2)
if arguments.mode == 'crashed_process_tree_image_tagging':
    worker = start_worker(2)
    os._exit(1)
if arguments.mode == 'runtime_path_image_tagging':
    subprocess.run(['runtime-helper'], check=True)
    if os.environ.get('RUNTIME_TEST_SETTING') != 'enabled':
        raise RuntimeError('runtime environment was not applied')

active_requests = 0
maximum_active_requests = 0
request_lock = threading.Lock()

def wait_for_concurrent_requests(expected):
    deadline = time.time() + 2.0
    while time.time() < deadline:
        with request_lock:
            if active_requests >= expected:
                return
        time.sleep(0.01)

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
        global active_requests, maximum_active_requests
        if arguments.mode == 'ocr' and self.path == '/v1/chat/completions':
            content_length = int(self.headers.get('Content-Length', '0'))
            request = json.loads(self.rfile.read(content_length))
            image_url = request['messages'][0]['content'][1]['image_url']['url']
            if not image_url.startswith('file://'):
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
        if set(descriptor) != {'jobId', 'sequence', 'byteSize', 'contentHash', 'mimeType', 'inputFilename'}:
            self.send_json(400, {'detail': 'invalid input descriptor'})
            return
        if 'screenshot_detection' in arguments.mode or 'document_detection' in arguments.mode or 'classifier' in arguments.mode:
            with request_lock:
                active_requests += 1
                maximum_active_requests = max(maximum_active_requests, active_requests)
            if arguments.mode == 'slow_screenshot_detection':
                wait_for_concurrent_requests(8)
            response = {'detected': True, 'confidence': 0.87}
            if arguments.mode == 'ordered_classifier':
                response = {
                    'detected': descriptor['sequence'] != 0,
                    'confidence': 0.2 if descriptor['sequence'] == 0 else 0.9,
                }
            if arguments.mode == 'malformed_classifier_confidence':
                response['confidence'] = 1.1
            if arguments.mode == 'malformed_classifier_extra':
                response['extra'] = True
            if arguments.mode == 'malformed_classifier_detected':
                response['detected'] = 'true'
            self.send_json(200, response)
            with request_lock:
                active_requests -= 1
            return
        if 'image_aesthetics' in arguments.mode:
            with request_lock:
                active_requests += 1
                maximum_active_requests = max(maximum_active_requests, active_requests)
            if arguments.mode == 'slow_image_aesthetics':
                wait_for_concurrent_requests(8)
            scores = {
                'aestheticScore': 0.81,
                'scenicScore': 0.72,
                'simplicityScore': 0.63,
                'landscapeScore': 0.54,
                'technicalQualityScore': 0.45,
            }
            malformed_prefix = 'malformed_image_aesthetics_'
            if arguments.mode.startswith(malformed_prefix):
                scores[arguments.mode[len(malformed_prefix):]] = 1.1
            self.send_json(200, scores)
            with request_lock:
                active_requests -= 1
            return
        if arguments.mode in ('image_tagging', 'process_tree_image_tagging', 'runtime_path_image_tagging'):
            self.send_json(200, {'tags': ['person', 'bicycle']})
            return
        if arguments.mode in ('face_detection', 'slow_face_detection', 'rolling_face_detection'):
            with request_lock:
                active_requests += 1
                maximum_active_requests = max(maximum_active_requests, active_requests)
            if arguments.mode == 'slow_face_detection':
                wait_for_concurrent_requests(8)
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
                'faceSizeScore': 0.8,
                'frontalityScore': 0.9,
                'visibilityScore': 0.85,
                'featureClarityScore': 0.75,
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
                'faceSizeScore': 0.8,
                'frontalityScore': 0.9,
                'visibilityScore': 0.85,
                'featureClarityScore': 0.75,
                'embedding': '',
                'embeddingEncoding': 'float32_le',
                'embeddingDimensions': 512,
            }]})
            return
        embedding = [1.0] + [0.0] * 767
        encoded = base64.b64encode(struct.pack('<768f', *embedding)).decode('ascii')
        if arguments.mode == 'malformed_clustering':
            encoded = base64.b64encode(struct.pack('<f', 1.0)).decode('ascii')
        self.send_json(200, {
            'embedding': encoded,
            'embeddingEncoding': 'float32_le',
            'embeddingDimensions': 768,
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
    static USED_PORTS: OnceLock<Mutex<HashSet<u16>>> = OnceLock::new();
    let used_ports = USED_PORTS.get_or_init(|| Mutex::new(HashSet::new()));
    loop {
        let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to reserve test port");
        let port = listener
            .local_addr()
            .expect("Failed to read test port")
            .port();
        if used_ports.lock().expect("test port registry").insert(port) {
            return port;
        }
    }
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
) -> (ServiceConfig, RuntimeSpec) {
    let mut arguments = vec![
        script_path.to_string_lossy().into_owned(),
        "--mode".to_string(),
        mode.to_string(),
        "--port".to_string(),
        port.to_string(),
        "--start-log".to_string(),
        start_log.to_string_lossy().into_owned(),
        "--input-root".to_string(),
        "{input_root}".to_string(),
        "--mount-source".to_string(),
        "{input_root}".to_string(),
        "--cache-dir".to_string(),
        "{cache_dir}".to_string(),
    ];
    if model_type == "face_detection" {
        arguments.extend([
            "--processing-concurrency".to_string(),
            "{processing_concurrency}".to_string(),
            "--model-concurrency".to_string(),
            "{model_concurrency}".to_string(),
            "--face-detection-size".to_string(),
            "{face_detection_size}".to_string(),
            "--recognition-batch-size".to_string(),
            "{recognition_batch_size}".to_string(),
            "--recognition-batch-wait-milliseconds".to_string(),
            "{recognition_batch_wait_milliseconds}".to_string(),
            "--minimum-face-likelihood".to_string(),
            "{minimum_face_likelihood}".to_string(),
            "--minimum-face-resolution-pixels".to_string(),
            "{minimum_face_resolution_pixels}".to_string(),
        ]);
    } else if matches!(model_type, "image_clustering" | "image_aesthetics") {
        arguments.extend([
            "--processing-concurrency".to_string(),
            "{processing_concurrency}".to_string(),
            "--model-concurrency".to_string(),
            "{model_concurrency}".to_string(),
            "--model-batch-wait-milliseconds".to_string(),
            "{model_batch_wait_milliseconds}".to_string(),
        ]);
    } else {
        arguments.extend([
            "--max-concurrent-jobs".to_string(),
            "{model_concurrency}".to_string(),
        ]);
    }
    let model_version = if model_type == "ocr" {
        "unlimited_ocr".to_string()
    } else if model_type == "image_clustering" {
        "dinov2-base".to_string()
    } else if model_type == "image_aesthetics" {
        "clip-vit-b-32-laion-aesthetic-v1".to_string()
    } else if model_type == "face_detection" {
        "buffalo_l".to_string()
    } else if model_type == "screenshot_detection" {
        "screenshot-heuristics-v1".to_string()
    } else if model_type == "document_detection" {
        "document-heuristics-v1".to_string()
    } else {
        "ram++".to_string()
    };
    let service_type = ServiceType::from_task(model_type).expect("known test service");
    let is_screenshot_detection = model_type == "screenshot_detection";
    let is_document_detection = model_type == "document_detection";
    let is_face_detection = model_type == "face_detection";
    let is_image_clustering = model_type == "image_clustering";
    let is_image_aesthetics = model_type == "image_aesthetics";
    let uses_dynamic_batching = is_image_clustering || is_image_aesthetics;
    let uses_staged_concurrency = uses_dynamic_batching
        || is_face_detection
        || is_screenshot_detection
        || is_document_detection;
    (
        ServiceConfig {
            enabled: true,
            model_type: model_type.to_string(),
            startup_timeout_seconds: 5,
            request_timeout_seconds: 5,
            max_tokens: 8192,
            minimum_face_likelihood: is_face_detection.then_some(0.8),
            minimum_face_resolution_pixels: is_face_detection.then_some(112),
            face_detection_size: is_face_detection.then_some(960),
            recognition_batch_size: is_face_detection.then_some(64),
            recognition_batch_wait_milliseconds: is_face_detection.then_some(5),
            model_batch_wait_milliseconds: uses_dynamic_batching.then_some(5),
            max_concurrent_jobs: (!uses_staged_concurrency).then_some(2),
            processing_concurrency: uses_staged_concurrency.then_some(2),
            model_concurrency: uses_staged_concurrency.then_some(2),
        },
        RuntimeSpec {
            service_type,
            executable: Path::new("python3").to_path_buf(),
            arguments,
            environment: Vec::new(),
            base_url: if model_type == "ocr" {
                format!("http://127.0.0.1:{port}/v1")
            } else {
                format!("http://127.0.0.1:{port}")
            },
            model: if model_type == "ocr" {
                "baidu/Unlimited-OCR".to_string()
            } else if model_type == "image_clustering" {
                "facebook/dinov2-base".to_string()
            } else if model_type == "image_aesthetics" {
                "ViT-B/32".to_string()
            } else if model_type == "face_detection" {
                "buffalo_l".to_string()
            } else {
                String::new()
            },
            model_version,
            embedding_dimensions: if model_type == "image_clustering" {
                768
            } else if model_type == "face_detection" {
                512
            } else {
                0
            },
        },
    )
}

#[test]
fn image_aesthetics_is_a_registered_service_type() {
    assert_eq!(
        ServiceType::from_task("image_aesthetics").expect("registered aesthetics task"),
        ServiceType::ImageAesthetics
    );
    assert_eq!(ServiceType::ImageAesthetics.as_str(), "image_aesthetics");
}

#[test]
fn classifier_tasks_are_registered_service_types() {
    assert_eq!(
        ServiceType::from_task("screenshot_detection").expect("screenshot task"),
        ServiceType::ScreenshotDetection
    );
    assert_eq!(
        ServiceType::from_task("document_detection").expect("document task"),
        ServiceType::DocumentDetection
    );
    assert_eq!(
        ServiceType::ScreenshotDetection.as_str(),
        "screenshot_detection"
    );
    assert_eq!(
        ServiceType::DocumentDetection.as_str(),
        "document_detection"
    );
}

pub(super) fn manager(services: Vec<(ServiceConfig, RuntimeSpec)>) -> ServiceManager {
    let fixture_root = Path::new(&services[0].1.arguments[0])
        .parent()
        .expect("fixture script parent")
        .to_path_buf();
    fs::create_dir_all(fixture_root.join("llm/queue/processing")).expect("test processing queue");
    let (services, runtimes): (Vec<_>, Vec<_>) = services.into_iter().unzip();
    ServiceManager::with_runtime_catalog(
        Arc::new(Config {
            server: ServerConfig {
                data_dir: fixture_root,
                ..ServerConfig::default()
            },
            scheduler: Default::default(),
            service: services,
        }),
        RuntimeCatalog::new(runtimes),
    )
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

    assert_eq!(first.embedding_dimensions, Some(768));
    assert_eq!(first.embedding_encoding.as_deref(), Some("float32_le"));
    assert!(first
        .embedding
        .as_ref()
        .is_some_and(|embedding| !embedding.is_empty()));
    assert_eq!(first.quality_score, Some(0.75));
    let serialized = serde_json::to_value(&first).expect("Response should serialize");
    assert_eq!(serialized["embeddingEncoding"], "float32_le");
    assert_eq!(serialized["embeddingDimensions"], 768);
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
async fn manager_reuses_classifier_runtime_and_switches_classifier_tasks() {
    let (_directory, script_path, start_log) = fixture();
    let screenshot_port = available_port();
    let document_port = available_port();
    let screenshot = service(
        "screenshot_detection",
        "screenshot_detection",
        screenshot_port,
        &script_path,
        &start_log,
    );
    let document = service(
        "document_detection",
        "document_detection",
        document_port,
        &script_path,
        &start_log,
    );
    let mut manager = manager(vec![screenshot, document]);

    let first = infer_one(
        &mut manager,
        "screenshot_detection",
        b"first image",
        "first.jpg",
    )
    .await
    .expect("first screenshot classification");
    let second = infer_one(
        &mut manager,
        "screenshot_detection",
        b"second image",
        "second.jpg",
    )
    .await
    .expect("reused screenshot classifier");

    assert_eq!(first.detected, Some(true));
    assert_eq!(first.confidence, Some(0.87));
    assert_eq!(second.model_version, "screenshot-heuristics-v1");
    assert_eq!(manager.active_task(), Some("screenshot_detection"));
    assert_eq!(
        fs::read_to_string(&start_log).expect("runtime starts"),
        "screenshot_detection:2\n"
    );

    let document_response = infer_one(
        &mut manager,
        "document_detection",
        b"document image",
        "document.jpg",
    )
    .await
    .expect("document classifier switch");

    assert_eq!(document_response.task, "document_detection");
    assert_eq!(document_response.model_type, "document_detection");
    assert_eq!(document_response.model_version, "document-heuristics-v1");
    assert_eq!(manager.active_task(), Some("document_detection"));
    assert_eq!(
        fs::read_to_string(&start_log).expect("runtime starts"),
        "screenshot_detection:2\ndocument_detection:2\n"
    );
    manager.shutdown().await.expect("runtime shutdown");
}

#[tokio::test]
async fn manager_rejects_malformed_classifier_contracts() {
    for mode in [
        "malformed_classifier_confidence",
        "malformed_classifier_extra",
        "malformed_classifier_detected",
    ] {
        let (_directory, script_path, start_log) = fixture();
        let classifier = service(
            "screenshot_detection",
            mode,
            available_port(),
            &script_path,
            &start_log,
        );
        let mut manager = manager(vec![classifier]);

        let error = infer_one(&mut manager, "screenshot_detection", b"image", "image.jpg")
            .await
            .expect_err("malformed classifier response must fail");

        assert!(
            error.to_string().contains("screenshot_detection"),
            "{error}"
        );
        manager.shutdown().await.expect("runtime shutdown");
    }
}

#[tokio::test]
async fn manager_preserves_classifier_results_for_every_ordered_input() {
    let (directory, script_path, start_log) = fixture();
    let classifier = service(
        "document_detection",
        "ordered_classifier",
        available_port(),
        &script_path,
        &start_log,
    );
    let mut manager = manager(vec![classifier]);
    let paths = [
        directory.path().join("document-0.jpg"),
        directory.path().join("document-1.jpg"),
    ];
    for path in &paths {
        fs::write(path, b"image").expect("classifier input");
    }
    let dispatcher = manager
        .dispatcher("document_detection")
        .await
        .expect("document dispatcher");

    let responses = dispatcher
        .infer_inputs(
            paths
                .into_iter()
                .enumerate()
                .map(|(sequence, path)| InferenceInput {
                    job_id: "abcdef12".to_string(),
                    sequence: sequence as u32,
                    frame_timestamp_ms: Some(sequence as i64 * 1000),
                    path,
                    byte_size: 5,
                    content_hash: format!("{:x}", Sha256::digest(b"image")),
                    mime_type: "image/jpeg".to_string(),
                    filename: format!("document-{sequence}.jpg"),
                })
                .collect(),
        )
        .await
        .expect("ordered classifier responses");

    assert_eq!(responses.len(), 2);
    assert_eq!(responses[0].sequence, 0);
    assert_eq!(responses[0].response.detected, Some(false));
    assert_eq!(responses[0].response.confidence, Some(0.2));
    assert_eq!(responses[1].sequence, 1);
    assert_eq!(responses[1].response.detected, Some(true));
    assert_eq!(responses[1].response.confidence, Some(0.9));
    manager.shutdown().await.expect("runtime shutdown");
}

#[tokio::test]
async fn manager_dispatches_classifier_jobs_without_a_provider_concurrency_limit() {
    let (directory, script_path, start_log) = fixture();
    let port = available_port();
    let classifier = service(
        "screenshot_detection",
        "slow_screenshot_detection",
        port,
        &script_path,
        &start_log,
    );
    let mut manager = manager(vec![classifier]);
    let inputs = (0..8)
        .map(|sequence| {
            let path = directory.path().join(format!("screenshot-{sequence}.jpg"));
            fs::write(&path, b"image").expect("classifier input");
            InferenceInput {
                job_id: format!("abcd34{sequence:02x}"),
                sequence,
                frame_timestamp_ms: None,
                path,
                byte_size: 5,
                content_hash: format!("{:x}", Sha256::digest(b"image")),
                mime_type: "image/jpeg".to_string(),
                filename: format!("screenshot-{sequence}.jpg"),
            }
        })
        .collect::<Vec<_>>();
    let dispatcher = manager
        .dispatcher("screenshot_detection")
        .await
        .expect("screenshot dispatcher");
    let mut in_flight = inputs
        .into_iter()
        .map(|input| dispatcher.infer_inputs(vec![input]))
        .collect::<FuturesUnordered<_>>();
    let mut responses = Vec::new();
    while let Some(response) = in_flight.next().await {
        responses.push(response);
    }
    let metrics: serde_json::Value = reqwest::get(format!("http://127.0.0.1:{port}/metrics"))
        .await
        .expect("metrics response")
        .json()
        .await
        .expect("metrics body");

    assert_eq!(responses.len(), 8);
    assert!(responses.iter().all(Result::is_ok));
    assert_eq!(metrics["maximumActiveRequests"], 8);
    manager.shutdown().await.expect("runtime shutdown");
}

#[tokio::test]
async fn manager_shutdown_terminates_the_runtime_process_group() {
    let (_directory, script_path, start_log) = fixture();
    let port = available_port();
    let tagging = service(
        "image_tagging",
        "process_tree_image_tagging",
        port,
        &script_path,
        &start_log,
    );
    let mut manager = manager(vec![tagging]);

    infer_one(&mut manager, "image_tagging", b"image", "image.jpg")
        .await
        .expect("tagging request");
    let worker_pid = fs::read_to_string(&start_log)
        .expect("runtime log")
        .lines()
        .find_map(|line| line.strip_prefix("worker:"))
        .expect("worker pid")
        .parse::<i32>()
        .expect("numeric worker pid");

    manager.shutdown().await.expect("runtime shutdown");

    for _ in 0..100 {
        let result = unsafe { libc::kill(worker_pid, 0) };
        if result == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("runtime worker {worker_pid} survived process-group shutdown");
}

#[tokio::test]
async fn manager_cleans_up_process_group_after_runtime_leader_exits() {
    let (_directory, script_path, start_log) = fixture();
    let port = available_port();
    let tagging = service(
        "image_tagging",
        "crashed_process_tree_image_tagging",
        port,
        &script_path,
        &start_log,
    );
    let mut manager = manager(vec![tagging]);

    let activation = manager.dispatcher("image_tagging").await;
    assert!(
        activation.is_err(),
        "crashed runtime should fail activation"
    );
    let worker_pid = fs::read_to_string(&start_log)
        .expect("runtime log")
        .lines()
        .find_map(|line| line.strip_prefix("worker:"))
        .expect("worker pid")
        .parse::<i32>()
        .expect("numeric worker pid");

    for _ in 0..100 {
        let result = unsafe { libc::kill(worker_pid, 0) };
        if result == -1 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH) {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("runtime worker {worker_pid} survived leader cleanup");
}

#[tokio::test]
async fn manager_applies_the_runtime_environment_and_executable_path() {
    let (directory, script_path, start_log) = fixture();
    let runtime_bin = directory.path().join("runtime-bin");
    fs::create_dir(&runtime_bin).expect("runtime bin directory");
    let python = runtime_bin.join("python");
    fs::write(&python, "#!/bin/sh\nexec /usr/bin/python3 \"$@\"\n").expect("python wrapper");
    fs::set_permissions(&python, fs::Permissions::from_mode(0o755)).expect("python permissions");
    let helper = runtime_bin.join("runtime-helper");
    fs::write(&helper, "#!/bin/sh\nexit 0\n").expect("runtime helper");
    fs::set_permissions(&helper, fs::Permissions::from_mode(0o755)).expect("helper permissions");
    let port = available_port();
    let (config, mut runtime) = service(
        "image_tagging",
        "runtime_path_image_tagging",
        port,
        &script_path,
        &start_log,
    );
    runtime.executable = python;
    runtime.environment = vec![("RUNTIME_TEST_SETTING".to_string(), "enabled".to_string())];
    let mut manager = manager(vec![(config, runtime)]);

    let response = infer_one(&mut manager, "image_tagging", b"image", "image.jpg")
        .await
        .expect("runtime helper should resolve from the executable directory");

    assert_eq!(response.tags, vec!["person", "bicycle"]);
    manager.shutdown().await.expect("runtime shutdown");
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

    assert!(error.to_string().contains("expected 3072"));
    manager
        .shutdown()
        .await
        .expect("Runtime should stop cleanly");
}

#[tokio::test]
async fn manager_serializes_valid_image_aesthetics_scores() {
    let (_directory, script_path, start_log) = fixture();
    let port = available_port();
    let aesthetics = service(
        "image_aesthetics",
        "image_aesthetics",
        port,
        &script_path,
        &start_log,
    );
    let mut manager = manager(vec![aesthetics]);

    let response = infer_one(&mut manager, "image_aesthetics", b"image", "image.jpg")
        .await
        .expect("Image aesthetics should succeed");

    assert_eq!(response.task, "image_aesthetics");
    assert_eq!(response.model_type, "image_aesthetics");
    assert_eq!(response.model_version, "clip-vit-b-32-laion-aesthetic-v1");
    assert_eq!(response.aesthetic_score, Some(0.81));
    assert_eq!(response.scenic_score, Some(0.72));
    assert_eq!(response.simplicity_score, Some(0.63));
    assert_eq!(response.landscape_score, Some(0.54));
    assert_eq!(response.technical_quality_score, Some(0.45));
    assert_eq!(manager.active_name(), "clip-aesthetic");
    let serialized = serde_json::to_value(response).expect("Aesthetics response should serialize");
    assert!(
        (serialized["aestheticScore"]
            .as_f64()
            .expect("serialized aesthetic score")
            - 0.81)
            .abs()
            < 0.000_001
    );
    assert!(
        (serialized["technicalQualityScore"]
            .as_f64()
            .expect("serialized technical quality score")
            - 0.45)
            .abs()
            < 0.000_001
    );
    assert_eq!(
        fs::read_to_string(&start_log).expect("runtime starts"),
        "image_aesthetics:2\n"
    );
    manager.shutdown().await.expect("runtime shutdown");
}

#[tokio::test]
async fn manager_rejects_each_invalid_image_aesthetics_score() {
    for (field, expected_name) in [
        ("aestheticScore", "aesthetic score"),
        ("scenicScore", "scenic score"),
        ("simplicityScore", "simplicity score"),
        ("landscapeScore", "landscape score"),
        ("technicalQualityScore", "technical quality score"),
    ] {
        let (_directory, script_path, start_log) = fixture();
        let port = available_port();
        let aesthetics = service(
            "image_aesthetics",
            &format!("malformed_image_aesthetics_{field}"),
            port,
            &script_path,
            &start_log,
        );
        let mut manager = manager(vec![aesthetics]);

        let error = infer_one(&mut manager, "image_aesthetics", b"image", "image.jpg")
            .await
            .expect_err("Out-of-range aesthetics score must be rejected");

        assert!(error.to_string().contains(expected_name), "{error}");
        manager.shutdown().await.expect("runtime shutdown");
    }
}

#[tokio::test]
async fn manager_dispatches_all_aesthetics_jobs_without_a_provider_concurrency_limit() {
    let (directory, script_path, start_log) = fixture();
    let port = available_port();
    let aesthetics = service(
        "image_aesthetics",
        "slow_image_aesthetics",
        port,
        &script_path,
        &start_log,
    );
    let mut manager = manager(vec![aesthetics]);
    let inputs = (0..8)
        .map(|sequence| {
            let path = directory.path().join(format!("aesthetic-{sequence}.jpg"));
            fs::write(&path, b"image").expect("queued aesthetics input");
            InferenceInput {
                job_id: format!("abcd12{sequence:02x}"),
                sequence,
                frame_timestamp_ms: None,
                path,
                byte_size: 5,
                content_hash: format!("{:x}", Sha256::digest(b"image")),
                mime_type: "image/jpeg".to_string(),
                filename: format!("aesthetic-{sequence}.jpg"),
            }
        })
        .collect::<Vec<_>>();

    let dispatcher = manager
        .dispatcher("image_aesthetics")
        .await
        .expect("aesthetics dispatcher");
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
    assert_eq!(first.faces[0].face_size_score, 0.8);
    assert_eq!(first.faces[0].visibility_score, 0.85);
    assert_eq!(first.faces[0].feature_clarity_score, 0.75);
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
