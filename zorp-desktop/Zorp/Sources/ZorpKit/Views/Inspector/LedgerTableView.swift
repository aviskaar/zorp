import SwiftUI

public struct LedgerTableView: View {
    public let experiments: [ExperimentFrame]

    public init(experiments: [ExperimentFrame]) {
        self.experiments = experiments
    }

    public var body: some View {
        Table(experiments) {
            TableColumn("Status") { exp in
                HStack(spacing: 4) {
                    Circle()
                        .fill(exp.status == "approved" ? Color.green : Color.red)
                        .frame(width: 8, height: 8)
                    Text(exp.status)
                        .font(.caption)
                }
            }
            .width(min: 70, max: 90)

            TableColumn("Conditions") { exp in
                Text(exp.conditions.map { "\($0.key)=\($0.value)" }.joined(separator: ", "))
                    .font(.system(.caption2, design: .monospaced))
                    .lineLimit(1)
            }
            .width(min: 100, ideal: 140)

            TableColumn("Metrics") { exp in
                Text(exp.metrics.map { "\($0.key)=\($0.value)" }.joined(separator: ", "))
                    .font(.system(.caption2, design: .monospaced))
                    .lineLimit(1)
            }
            .width(min: 100, ideal: 140)
        }
    }
}
