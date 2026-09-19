import SwiftUI

public struct MarkdownView: View {
    public let text: String

    public init(_ text: String) {
        self.text = text
    }

    public var body: some View {
        Text(LocalizedStringKey(text))
            .textSelection(.enabled)
            .font(.system(.body, design: .default))
            .lineSpacing(4)
    }
}
