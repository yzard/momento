"""Disposable loopback-only API fixture for native iOS UI tests; never uses user data."""
import argparse
import base64
import json
import hashlib
import re
from collections import Counter
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from threading import Lock

USER = dict(id=1, username="ios-test", email="ios@example.invalid", role="admin", isReserved=False,
            mustChangePassword=False, isActive=True, createdAt="2026-01-01T00:00:00Z")
MEDIA = [dict(id=i, filename=f"photo-{i}.jpg", originalFilename=f"photo-{i}.jpg", mediaType="image",
              createdAt="2026-01-01T12:00:00Z", dateTaken="2026-01-01T12:00:00Z") for i in range(1, 13)]
PNG = base64.b64decode("iVBORw0KGgoAAAANSUhEUgAAAAIAAAACCAYAAABytg0kAAAAEUlEQVR4nGNQWLX7PwgzwBgAVUYKEZ3nZ48AAAAASUVORK5CYII=")


class Handler(BaseHTTPRequestHandler):
    counts = Counter()
    lock = Lock()
    schedules = [dict(feature=key, cronExpression="0 0 * * *") for key in ["ocr", "image_tagging", "screenshot_detection", "document_detection", "image_aesthetics", "deduplicate", "face_detection"]]
    albums = []
    uploads = {}
    users = [USER, {**USER, "id": 2, "username": "fixture-user", "email": "fixture@example.invalid", "role": "user"}]

    def log_message(self, *args):
        pass

    def reply(self, status, body):
        data = json.dumps(body).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        if self.path == "/metrics":
            with self.lock:
                return self.reply(200, dict(self.counts))
        if self.path == "/api/v1/client/capabilities":
            return self.reply(200, dict(appVersion="fixture", apiVersion=1, supportedMediaExtensions=[".png", ".mov"],
                features={key: True for key in ["llm", "imageTagging", "deduplicate", "faceDetection", "imageAesthetics", "screenshotDetection", "documentDetection"]},
                backup=dict(enabled=True, protocolVersion=2, maxUploadBytes=100000000, maxChunkBytes=32, sessionExpiryHours=24, batch=dict(version=1, maxBytes=50000000, maxItems=256, maxManifestBytes=1048576))))
        if self.path == "/api/v1/media/10915/original":
            data = (Path(__file__).parents[2] / "common/resources/media/fragmented-preview.mp4").read_bytes()
            match = re.fullmatch(r"bytes=(\d+)-(\d*)", self.headers.get("Range", ""))
            start, end = (int(match[1]), min(int(match[2]) if match[2] else len(data) - 1, len(data) - 1)) if match else (0, len(data) - 1)
            if start >= len(data):
                self.send_response(416); self.send_header("Content-Range", f"bytes */{len(data)}"); self.end_headers(); return
            self.send_response(206 if match else 200)
            self.send_header("Content-Type", "video/mp4")
            self.send_header("Accept-Ranges", "bytes")
            if match: self.send_header("Content-Range", f"bytes {start}-{end}/{len(data)}")
            self.send_header("Content-Length", str(end - start + 1))
            self.end_headers(); self.wfile.write(data[start:end + 1]); return
        if "/thumbnail" in self.path or self.path.endswith("/preview") or self.path == "/fixture-original":
            self.send_response(200)
            self.send_header("Content-Type", "image/png")
            self.send_header("Content-Length", str(len(PNG)))
            self.end_headers()
            self.wfile.write(PNG)
            return
        self.reply(404, {"detail": "Unknown fixture route"})

    def do_POST(self):
        raw = self.rfile.read(int(self.headers.get("Content-Length", "0")))
        body = {} if self.path == "/api/v1/backup/upload/batch" else json.loads(raw or "{}")
        with self.lock:
            self.counts[self.path] += 1
        if self.path == "/api/v1/user/authenticate":
            return self.reply(200, dict(accessToken="fixture-access", refreshToken="fixture-refresh", tokenType="bearer"))
        if self.headers.get("Authorization") != "Bearer fixture-access":
            return self.reply(401, {"detail": "Missing test token"})
        if self.path == "/api/v1/user/get":
            return self.reply(200, USER)
        if self.path == "/api/v1/user/logout":
            return self.reply(200, {"message": "Signed out"})
        if self.path == "/api/v1/user/change-password":
            if body != {"currentPassword": "fixture-only", "newPassword": "fixture-replacement"}:
                return self.reply(400, {"detail": "Unexpected disposable password-change request"})
            return self.reply(200, {"message": "Password changed"})
        if self.path == "/api/v1/user/list":
            return self.reply(200, {"users": self.users})
        if self.path == "/api/v1/user/update":
            user = next((item for item in self.users if item["id"] == body.get("userId")), None)
            if user is None or user["id"] == USER["id"]:
                return self.reply(400, {"detail": "Expected the separate fixture user"})
            user.update({key: body[key] for key in ["role", "isActive"] if key in body})
            return self.reply(200, user)
        if self.path == "/api/v1/import/status":
            return self.reply(200, dict(status="idle", totalFiles=12, processedFiles=12, totalMedia=12, successfulImports=11, failedImports=1, startedAt=None, completedAt=None, errors=["Fixture import failure"]))
        if self.path == "/api/v1/metadata/status":
            return self.reply(200, dict(status="idle", queuedJobs=0, processingJobs=0, cancellingJobs=0, completedJobs=12, failedJobs=1, errors=["Fixture metadata failure"], waitingForRollbackJobs=2))
        if self.path == "/api/v1/ai/schedule/update":
            item = next(x for x in self.schedules if x["feature"] == body["feature"])
            if body != {"feature": "ocr", "cronExpression": "5 0 * * *"}:
                return self.reply(400, {"detail": "Unexpected fixture schedule edit"})
            item.update(cronExpression=body["cronExpression"])
            return self.reply(200, item)
        if self.path == "/api/v1/backup/device/register":
            return self.reply(200, {"registered": True})
        if self.path == "/api/v1/backup/upload/batch/prepare":
            results = []
            for item in body["uploads"]:
                upload_id = "batch-" + item["operationId"]
                if upload_id not in self.uploads:
                    self.uploads[upload_id] = dict(uploadId=upload_id, status="uploading", uploadedSize=0,
                        expectedSize=item["byteSize"], contentHash=item["contentHash"], data=bytearray())
                results.append(dict(key=item["operationId"], upload=self.upload_value(upload_id), error=None))
            return self.reply(200, dict(results=results))
        if self.path == "/api/v1/backup/upload/batch/status":
            return self.reply(200, dict(results=[dict(key=key, upload=self.upload_value(key), error=None) for key in body["uploadIds"]]))
        if self.path == "/api/v1/backup/upload/batch":
            length = int.from_bytes(raw[:4], "big")
            manifest = json.loads(raw[4:4+length])
            position = 4 + length
            results = []
            for entry in manifest["entries"]:
                upload = self.uploads[entry["uploadId"]]
                data = raw[position:position+entry["byteSize"]]
                position += entry["byteSize"]
                if len(data) != upload["expectedSize"] or hashlib.sha256(data).hexdigest() != upload["contentHash"]:
                    results.append(dict(key=entry["uploadId"], upload=None, error=dict(status=400, detail="Original hash mismatch")))
                else:
                    upload.update(status="completed", uploadedSize=len(data), data=bytearray(data), mediaId=100)
                    results.append(dict(key=entry["uploadId"], upload=self.upload_value(entry["uploadId"]), error=None))
            if position != len(raw):
                return self.reply(400, {"detail": "Unexpected batch length"})
            return self.reply(200, dict(results=results))
        if self.path == "/api/v1/backup/upload/create":
            upload_id = str(len(self.uploads) + 1)
            self.uploads[upload_id] = dict(uploadId=upload_id, status="uploading", uploadedSize=0,
                expectedSize=body["byteSize"], contentHash=body["contentHash"], data=bytearray())
            return self.upload_reply(upload_id)
        if self.path in ["/api/v1/backup/upload/status", "/api/v1/backup/upload/complete", "/api/v1/backup/upload/cancel"]:
            upload_id = body["uploadId"]
            upload = self.uploads[upload_id]
            if self.path.endswith("/complete"):
                if len(upload["data"]) != upload["expectedSize"] or hashlib.sha256(upload["data"]).hexdigest() != upload["contentHash"]:
                    return self.reply(400, {"detail": "Original bytes do not match creation hash"})
                upload["status"] = "completed"
                upload["mediaId"] = 100
            elif self.path.endswith("/cancel"):
                upload["status"] = "cancelled"
            return self.upload_reply(upload_id)
        if self.path == "/api/v1/timeline/list":
            if not body.get("anchorDate"):
                return self.reply(400, {"detail": "initial timeline requests require anchorDate"})
            return self.reply(200, dict(groups=[dict(date="2026-01-01", media=([dict(id=10915, filename="fragmented-preview.mp4", originalFilename="fragmented-preview.mp4", mediaType="video", mimeType="video/mp4", durationSeconds=10, createdAt="2026-01-01T12:00:00Z")] if body.get("mediaType") == "video" else MEDIA))], nextCursor=None, previousCursor=None, hasOlder=False, hasNewer=False))
        if self.path == "/api/v1/album/list":
            return self.reply(200, {"albums": self.albums})
        if self.path == "/api/v1/album/create":
            detail = dict(id=1, name=body["name"], description=body.get("description"), coverMediaId=None, media=[], createdAt="2026-01-01")
            self.albums.append({**detail, "thumbnailMediaIds": [], "mediaCount": 0})
            return self.reply(200, detail)
        if self.path == "/api/v1/album/get":
            return self.reply(200, self.albums[0])
        if self.path == "/api/v1/album/update":
            self.albums[0].update({k: v for k, v in body.items() if k != "albumId"})
            return self.reply(200, self.albums[0])
        if self.path == "/api/v1/places/list":
            return self.reply(200, {"places": [], "nextCursor": None, "hasMore": False})
        if self.path == "/api/v1/faces/groups/list":
            return self.reply(200, {"groups": [dict(faceGroupId=i, faceCount=3, mediaCount=3) for i in [1, 2]], "nextCursor": None, "hasMore": False})
        if self.path == "/api/v1/duplicates/list":
            return self.reply(200, dict(groups=[dict(clusterId=1, items=MEDIA[:3])],
                nextCursor=None, hasMore=False, totalGroups=1, totalMedia=3))
        if self.path == "/api/v1/map/clusters":
            return self.reply(200, dict(clusters=[dict(id="fixture", lat=40.7, lng=-74,
                count=len(MEDIA), representativeId=1)], totalCount=len(MEDIA)))
        if self.path == "/api/v1/map/media":
            return self.reply(200, dict(items=MEDIA))
        if self.path == "/api/v1/ai/status":
            jobs = dict(queued=0, submitting=0, submitted=0, completed=0, failed=0, cancelled=0)
            return self.reply(200, dict(tasks=[dict(task=x["feature"], enabled=True, state="idle", jobs=jobs, errors=[]) for x in self.schedules if x["feature"] != "deduplicate"], schedules=self.schedules, faceGroups=2,
                deduplicate=dict(status="idle", ensembledMedia=0, processedMedia=0,
                    candidateComparisons=0, clustersCreated=1, jobs=jobs)))
        if self.path == "/api/v1/trash/list":
            return self.reply(200, {"items": [], "totalCount": 0})
        self.reply(404, {"detail": "Unknown fixture route: " + self.path})

    def upload_value(self, upload_id):
        return {key: value for key, value in self.uploads[upload_id].items() if key != "data"}

    def upload_reply(self, upload_id):
        self.reply(200, self.upload_value(upload_id))

    def do_PUT(self):
        raw = self.rfile.read(int(self.headers.get("Content-Length", "0")))
        if self.headers.get("Authorization") != "Bearer fixture-access" or not self.path.startswith("/api/v1/backup/upload/chunk/"):
            return self.reply(401, {"detail": "Invalid fixture upload"})
        upload_id = self.path.rsplit("/", 1)[1]
        upload = self.uploads[upload_id]
        match = re.fullmatch(r"bytes (\d+)-(\d+)/(\d+)", self.headers.get("Content-Range", ""))
        expected = (len(upload["data"]), len(upload["data"]) + len(raw) - 1, upload["expectedSize"])
        if not match or tuple(map(int, match.groups())) != expected or hashlib.sha256(raw).hexdigest() != self.headers.get("X-Content-SHA256"):
            return self.reply(400, {"detail": "Invalid range or chunk hash"})
        upload["data"].extend(raw)
        upload["uploadedSize"] = len(upload["data"])
        with self.lock:
            self.counts[self.path] += 1
        self.upload_reply(upload_id)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--port-file", type=Path, required=True)
    args = parser.parse_args()
    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    args.port_file.write_text(str(server.server_address[1]))
    try:
        server.serve_forever()
    finally:
        server.server_close()
