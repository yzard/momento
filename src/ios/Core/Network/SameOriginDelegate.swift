import Foundation

/// Never forward authenticated requests to a different origin, including redirects to HTTP.
final class SameOriginDelegate: NSObject, URLSessionTaskDelegate, Sendable {
    private let progress: (@Sendable (Int64) -> Void)?
    init(progress: (@Sendable (Int64) -> Void)?) { self.progress = progress }

    func urlSession(_ session: URLSession, task: URLSessionTask, didSendBodyData bytesSent: Int64,
                    totalBytesSent: Int64, totalBytesExpectedToSend: Int64) {
        progress?(totalBytesSent)
    }
    func urlSession(_ session: URLSession, task: URLSessionTask, willPerformHTTPRedirection response: HTTPURLResponse,
                    newRequest request: URLRequest, completionHandler: @escaping @Sendable (URLRequest?) -> Void) {
        guard let original = task.originalRequest?.url, let next = request.url,
              original.scheme == next.scheme, original.host == next.host, original.port == next.port else {
            completionHandler(nil); return
        }
        completionHandler(request)
    }
}
