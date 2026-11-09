import SwiftUI

struct AdminPanel<Content: View>: View {
    let title: String
    let description: String
    @ViewBuilder let content: () -> Content
    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(title).font(.title2.weight(.semibold))
            Text(description).font(.subheadline).foregroundStyle(.secondary)
            content()
        }.frame(maxWidth: .infinity, alignment: .leading).padding(20)
            .background(Color(uiColor: .secondarySystemGroupedBackground), in: .rect(cornerRadius: 18))
    }
}

struct AdminMetric: Identifiable {
    let label: String
    let value: String
    let failed: Bool
    var id: String { label }
}

struct AdminMetrics: View {
    let metrics: [AdminMetric]
    var body: some View {
        LazyVGrid(columns: [GridItem(.flexible()), GridItem(.flexible())], alignment: .leading, spacing: 10) {
            ForEach(metrics) { metric in
                VStack(alignment: .leading, spacing: 6) {
                    Text(metric.label.uppercased()).font(.caption).foregroundStyle(.secondary)
                    Text(metric.value).font(.title2.monospaced().bold()).foregroundStyle(metric.failed ? Color.red : .primary)
                }.frame(maxWidth: .infinity, minHeight: 64, alignment: .topLeading).padding(12)
                    .background(Color(uiColor: .tertiarySystemGroupedBackground), in: .rect(cornerRadius: 12))
            }
        }
    }
}

struct AdminFailureLog: View {
    let title: String
    let entries: [String]
    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(title.uppercased()).font(.caption.bold()).foregroundStyle(.secondary)
            ScrollView { Text(entries.isEmpty ? "No failures." : entries.joined(separator: "\n"))
                    .font(.caption.monospaced()).frame(maxWidth: .infinity, alignment: .topLeading).padding(14).textSelection(.enabled)
            }.frame(height: 112).background(Color(uiColor: .tertiarySystemGroupedBackground), in: .rect(cornerRadius: 12))
        }.padding(.top, 8)
    }
}

struct AdminTable<Content: View>: View {
    let headers: [String]
    let widths: [CGFloat]
    let alignments: [Alignment]
    @ViewBuilder let content: () -> Content
    var body: some View {
        ScrollView(.horizontal) {
            VStack(spacing: 0) {
                HStack(spacing: 0) {
                    ForEach(headers.indices, id: \.self) { i in
                        Text(headers[i]).font(.caption.bold()).frame(maxWidth: .infinity, alignment: alignments[i]).adminCell(widths[i])
                    }
                }.background(Color(uiColor: .tertiarySystemGroupedBackground))
                content()
            }.frame(width: widths.reduce(0, +)).background(Color(uiColor: .secondarySystemGroupedBackground))
                .clipShape(.rect(cornerRadius: 12)).overlay { RoundedRectangle(cornerRadius: 12).stroke(.quaternary) }
        }.scrollIndicators(.visible)
    }
}

extension View {
    func adminCell(_ width: CGFloat) -> some View {
        padding(12).frame(width: width, alignment: .leading)
    }
}

enum AdminFeature: String, CaseIterable, Identifiable {
    case ocr, imageTagging = "image_tagging", screenshotDetection = "screenshot_detection"
    case documentDetection = "document_detection", imageAesthetics = "image_aesthetics"
    case deduplicate, faceDetection = "face_detection"
    var id: String { rawValue }
    var title: String {
        switch self {
        case .ocr: "OCR"
        case .imageTagging: "Image tagging"
        case .screenshotDetection: "Screenshot detection"
        case .documentDetection: "Document detection"
        case .imageAesthetics: "Image aesthetics"
        case .deduplicate: "Deduplication"
        case .faceDetection: "Face detection"
        }
    }
    func jobs(_ status: AiStatusResponse?) -> AiJobCounts? {
        self == .deduplicate ? status?.deduplicate.jobs : status?.tasks.first { $0.task == rawValue }?.jobs
    }
    func state(_ status: AiStatusResponse?) -> String? {
        self == .deduplicate ? status?.deduplicate.status : status?.tasks.first { $0.task == rawValue }?.state
    }
}

struct AdminScheduleFields: View {
    @Environment(AppModel.self) private var model
    let schedule: AiFeatureSchedule?
    let feature: AdminFeature
    let saved: (AiFeatureSchedule) -> Void
    @State private var fields: [String]
    @FocusState private var focusedField: Int?
    private var cron: String { fields.map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }.joined(separator: " ") }
    init(schedule: AiFeatureSchedule?, feature: AdminFeature, saved: @escaping (AiFeatureSchedule) -> Void) {
        self.schedule = schedule; self.feature = feature; self.saved = saved
        let parsed = schedule?.cronExpression.split(whereSeparator: \.isWhitespace).map(String.init) ?? []
        _fields = State(initialValue: parsed.count == 5 ? parsed : Array(repeating: "", count: 5))
    }
    var body: some View {
        HStack(spacing: 0) {
            ForEach(Array(["Minute", "Hour", "Day", "Month", "Weekday"].enumerated()), id: \.offset) { i, label in
                TextField(label, text: $fields[i]).textInputAutocapitalization(.never).autocorrectionDisabled()
                    .multilineTextAlignment(.center).keyboardType(.asciiCapable).submitLabel(.done).focused($focusedField, equals: i)
                    .onSubmit { focusedField = nil }
                    .textFieldStyle(.roundedBorder).accessibilityLabel("\(feature.title) \(label)").adminCell(92)
            }
            Button { Task { await model.perform {
                let result: AiFeatureSchedule = try await model.api.call("ai/schedule/update", AiScheduleUpdateRequest(feature: feature.rawValue, cronExpression: cron))
                saved(result)
            } } } label: { Label("Save", systemImage: "square.and.arrow.down").labelStyle(.iconOnly) }
                .buttonStyle(.glass).accessibilityLabel("Save \(feature.title) cron schedule")
                .disabled(schedule == nil || !AdminPolicy.validCron(fields) || cron == schedule?.cronExpression || model.busy).adminCell(72)
        }.onChange(of: schedule?.cronExpression) { old, new in
            // Polling must not discard an unfinished local schedule edit.
            if old == nil || cron == old {
                let parsed = new?.split(whereSeparator: \.isWhitespace).map(String.init) ?? []
                if parsed.count == 5 { fields = parsed }
            }
        }
    }
}
