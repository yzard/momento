use llm_service::config::{Config, GeneralConfig, LoggingConfig, ProviderKind, ServiceConfig};
use llm_service::provider::ServiceManager;
use std::fs;
use std::net::TcpListener;
use std::path::Path;
use std::sync::Arc;
use tempfile::TempDir;

const MOCK_RUNTIME: &str = r#"
import argparse
import base64
import json
import struct
from http.server import BaseHTTPRequestHandler, HTTPServer

parser = argparse.ArgumentParser()
parser.add_argument('--mode', required=True)
parser.add_argument('--port', required=True, type=int)
parser.add_argument('--start-log', required=True)
arguments = parser.parse_args()

with open(arguments.start_log, 'a', encoding='utf-8') as output:
    output.write(arguments.mode + '\n')

class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path == '/ready':
            self.send_json(200, {'status': 'ready'})
            return
        self.send_error(404)

    def do_POST(self):
        if self.path != '/infer':
            self.send_error(404)
            return
        content_length = int(self.headers.get('Content-Length', '0'))
        self.rfile.read(content_length)
        if arguments.mode == 'image_tagging':
            self.send_json(200, {'tags': ['person', 'bicycle']})
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

HTTPServer(('127.0.0.1', arguments.port), Handler).serve_forever()
"#;

fn available_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to reserve test port");
    listener
        .local_addr()
        .expect("Failed to read test port")
        .port()
}

fn service(
    model_type: &str,
    mode: &str,
    port: u16,
    script_path: &Path,
    start_log: &Path,
) -> ServiceConfig {
    ServiceConfig {
        enabled: true,
        model_type: model_type.to_string(),
        model_version: if model_type == "image_clustering" {
            "dinov2-small".to_string()
        } else {
            "ram++".to_string()
        },
        provider: ProviderKind::Local,
        docker_command: vec![
            "python3".to_string(),
            "{script_path}".to_string(),
            "--mode".to_string(),
            mode.to_string(),
            "--port".to_string(),
            port.to_string(),
            "--start-log".to_string(),
            start_log.to_string_lossy().into_owned(),
        ],
        device: "cpu".to_string(),
        base_url: format!("http://127.0.0.1:{port}"),
        model: if model_type == "image_clustering" {
            "facebook/dinov2-small".to_string()
        } else {
            String::new()
        },
        script_path: script_path.to_string_lossy().into_owned(),
        api_key: String::new(),
        secret_key: String::new(),
        token_url: String::new(),
        ocr_url: String::new(),
        max_image_width: 0,
        max_image_height: 0,
        startup_timeout_seconds: 5,
        request_timeout_seconds: 5,
        max_tokens: 0,
        embedding_dimensions: if model_type == "image_clustering" {
            384
        } else {
            0
        },
    }
}

fn manager(services: Vec<ServiceConfig>) -> ServiceManager {
    ServiceManager::new(Arc::new(Config {
        general: GeneralConfig::default(),
        logging: LoggingConfig::default(),
        service: services,
    }))
}

fn fixture() -> (TempDir, std::path::PathBuf, std::path::PathBuf) {
    let directory = TempDir::new().expect("Failed to create runtime fixture");
    let script_path = directory.path().join("mock_runtime.py");
    let start_log = directory.path().join("starts.log");
    fs::write(&script_path, MOCK_RUNTIME).expect("Failed to write mock runtime");
    (directory, script_path, start_log)
}

#[tokio::test]
async fn manager_reuses_clustering_and_stops_it_before_switching() {
    let (_directory, script_path, start_log) = fixture();
    let port = available_port();
    let clustering = service(
        "image_clustering",
        "image_clustering",
        port,
        &script_path,
        &start_log,
    );
    let tagging = service(
        "image_tagging",
        "image_tagging",
        port,
        &script_path,
        &start_log,
    );
    let mut manager = manager(vec![clustering, tagging]);

    let first = manager
        .infer("image_clustering", b"first image", "first.jpg")
        .await
        .expect("First clustering request should succeed");
    let second = manager
        .infer("image_clustering", b"second image", "second.jpg")
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
        "image_clustering\n"
    );

    let tagging_response = manager
        .infer("image_tagging", b"tag image", "tag.jpg")
        .await
        .expect("Tagging should start after clustering stops");

    assert_eq!(tagging_response.tags, vec!["person", "bicycle"]);
    assert_eq!(manager.active_name(), "ram++");
    assert_eq!(
        fs::read_to_string(&start_log).expect("Failed to read runtime starts"),
        "image_clustering\nimage_tagging\n"
    );

    manager
        .shutdown()
        .await
        .expect("Runtime should stop cleanly");
    assert_eq!(manager.active_name(), "on-demand");
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

    let error = manager
        .infer("image_clustering", b"image", "image.jpg")
        .await
        .expect_err("Short embedding should be rejected");

    assert!(error.to_string().contains("expected 1536"));
    manager
        .shutdown()
        .await
        .expect("Runtime should stop cleanly");
}
