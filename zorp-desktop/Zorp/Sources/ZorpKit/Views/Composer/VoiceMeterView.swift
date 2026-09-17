import SwiftUI

public struct VoiceMeterView: View {
    public let level: Float

    public init(level: Float) {
        self.level = level
    }

    public var body: some View {
        HStack(spacing: 3) {
            ForEach(0..<8) { index in
                RoundedRectangle(cornerRadius: 1.5)
                    .fill(Float(index) / 8.0 <= level ? Color.accentColor : Color.secondary.opacity(0.3))
                    .frame(width: 3, height: CGFloat(8 + index * 2))
            }
        }
        .frame(height: 24)
    }
}
