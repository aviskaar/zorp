import SwiftUI

/// A review panel as it reports. Every reviewer read the same material
/// and none saw another's verdict; agreement is counted on the server, in
/// code, and shown as counted.
struct PanelRow: View {
    let run: PanelRun

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack {
                Image(systemName: "person.3")
                Text(run.target.map { "Review panel: \($0)" } ?? "Review panel").font(.headline)
                Spacer()
                if let verdicts = run.verdicts, let requested = run.lensesRequested {
                    Text("\(verdicts) of \(requested) reviewers answered")
                        .font(.caption)
                        .foregroundStyle(run.complete == true ? Color.secondary : Color.orange)
                }
            }
            if run.complete == false {
                Text("Partial panel. Agreement between fewer reviewers is a weaker claim than the count suggests.")
                    .font(.caption).foregroundStyle(.orange)
            }
            ForEach(Array(run.reviewers.enumerated()), id: \.offset) { _, reviewer in
                ReviewerRow(reviewer: reviewer)
            }
            if !run.agreements.isEmpty {
                Divider()
                Text("Raised by more than one lens").font(.subheadline).bold()
                ForEach(Array(run.agreements.enumerated()), id: \.offset) { _, agreement in
                    HStack(alignment: .firstTextBaseline) {
                        SeverityBadge(severity: agreement.highest)
                        Text(agreement.locus).font(.system(.callout, design: .monospaced))
                        Spacer()
                        Text(agreement.lenses.joined(separator: ", ")).font(.caption).foregroundStyle(.secondary)
                    }
                }
            } else if run.isDone {
                Text("No locus was raised by more than one lens.").font(.caption).foregroundStyle(.secondary)
            }
        }
        .padding(12)
        .background(Color.purple.opacity(0.05))
        .clipShape(RoundedRectangle(cornerRadius: 10))
    }
}

struct ReviewerRow: View {
    let reviewer: PanelReviewer
    @State private var expanded = false

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Button {
                expanded.toggle()
            } label: {
                HStack {
                    switch reviewer.state {
                    case .running: ProgressView().controlSize(.mini)
                    case .finished: Image(systemName: "checkmark.circle").foregroundStyle(.green)
                    case .failed: Image(systemName: "exclamationmark.circle").foregroundStyle(.red)
                    }
                    Text(reviewer.lens).bold()
                    if reviewer.state == .finished {
                        Text("\(reviewer.findings.count) finding\(reviewer.findings.count == 1 ? "" : "s")")
                            .foregroundStyle(.secondary)
                    }
                    if let why = reviewer.why {
                        Text(why).foregroundStyle(.red).lineLimit(1)
                    }
                    Spacer()
                    if reviewer.state == .finished {
                        Image(systemName: expanded ? "chevron.down" : "chevron.right").font(.caption2)
                    }
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .font(.callout)
            if expanded {
                ForEach(Array(reviewer.findings.enumerated()), id: \.offset) { _, finding in
                    HStack(alignment: .firstTextBaseline, spacing: 8) {
                        SeverityBadge(severity: finding.severity)
                        VStack(alignment: .leading, spacing: 2) {
                            Text(finding.locus).font(.system(.caption, design: .monospaced)).foregroundStyle(.secondary)
                            Text(finding.claim).font(.callout).textSelection(.enabled)
                        }
                    }
                }
                if !reviewer.answer.isEmpty {
                    DisclosureGroup("Full review (model-written)") {
                        MarkdownView(reviewer.answer)
                    }
                    .font(.caption)
                }
            }
        }
    }
}

struct SeverityBadge: View {
    let severity: String

    private var color: Color {
        switch severity {
        case "blocking": return .red
        case "concern": return .orange
        default: return .secondary
        }
    }

    var body: some View {
        Text(severity)
            .font(.caption2)
            .padding(.horizontal, 6)
            .padding(.vertical, 1)
            .background(color.opacity(0.15))
            .foregroundStyle(color)
            .clipShape(Capsule())
    }
}

/// A bolt run: where it is, then its verdict and what it recorded.
struct InvestigateRow: View {
    let run: InvestigateRun
    let session: SessionViewModel
    let onOpen: (String) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Image(systemName: "bolt.fill").foregroundStyle(.yellow)
                Text("Investigation").font(.headline)
                if let attempt = run.attempt, let of = run.of {
                    ProgressView(value: Double(attempt), total: Double(max(of, 1)))
                        .frame(width: 90)
                }
                Spacer()
                if !run.finished {
                    Button("Stop After This Attempt") {
                        Task { await session.stopAfterAttempt() }
                    }
                    .controlSize(.small)
                    .help("Let the running attempt finish and be recorded, then write the track up.")
                }
            }
            if let question = run.question {
                Text(question).font(.callout).foregroundStyle(.secondary)
            }
            HStack(spacing: 6) {
                if !run.finished { ProgressView().controlSize(.mini) }
                Text(run.verdict ?? run.line).font(.callout)
            }
            if run.needsPrereg {
                Text("Nothing committed a metric and a kill threshold for this question. Run it again with a pre-registration.")
                    .font(.caption).foregroundStyle(.orange)
            }
            if let ledger = run.ledger, !ledger.experiments.isEmpty {
                LedgerTable(ledger: ledger)
            }
            if let artifact = run.artifact {
                Button {
                    onOpen(artifact)
                } label: {
                    Label("Open the write-up: \(artifact)", systemImage: "doc.text")
                }
                .buttonStyle(.link)
            }
        }
        .padding(12)
        .background(Color.yellow.opacity(0.06))
        .clipShape(RoundedRectangle(cornerRadius: 10))
    }
}

/// What the aryabhatta ledger recorded for the track. Conditions, stated
/// expectations and observed metrics only: nothing in it is model prose.
struct LedgerTable: View {
    let ledger: LedgerFrame

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(ledger.forecasting
                ? "Forecasting is on: each attempt recorded an expectation before it ran."
                : "Forecasting is off for this server, so no attempt recorded an expectation. Set ZORP_FORECAST to turn it on.")
                .font(.caption).foregroundStyle(.secondary)
            ForEach(ledger.experiments) { experiment in
                VStack(alignment: .leading, spacing: 3) {
                    HStack {
                        Text(experiment.status).font(.caption).bold()
                        Spacer()
                        Text(String(experiment.id.prefix(8))).font(.caption2).foregroundStyle(.tertiary)
                    }
                    ForEach(experiment.metrics, id: \.key) { metric in
                        Text("\(metric.key) = \(metric.value)").font(.system(.caption, design: .monospaced))
                    }
                    ForEach(experiment.expectations, id: \.metricKey) { e in
                        Text("expected \(e.metricKey) in [\(fmt(e.intervalLow)), \(fmt(e.intervalHigh))] at \(Int(e.confidence * 100))%")
                            .font(.caption).foregroundStyle(.secondary)
                    }
                    if !experiment.conditions.isEmpty {
                        Text(experiment.conditions.map { "\($0.key)=\($0.value)" }.joined(separator: "  "))
                            .font(.caption2).foregroundStyle(.tertiary).lineLimit(2)
                    }
                }
                .padding(8)
                .background(Color.secondary.opacity(0.06))
                .clipShape(RoundedRectangle(cornerRadius: 6))
            }
        }
    }

    private func fmt(_ v: Double) -> String { String(format: "%g", v) }
}
