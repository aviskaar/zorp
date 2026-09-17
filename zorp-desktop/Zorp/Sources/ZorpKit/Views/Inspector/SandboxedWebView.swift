import SwiftUI
import WebKit

public struct SandboxedWebView: NSViewRepresentable {
    public let content: String
    public let isHTML: Bool

    public init(content: String, isHTML: Bool = true) {
        self.content = content
        self.isHTML = isHTML
    }

    public func makeNSView(context: Context) -> WKWebView {
        let config = WKWebViewConfiguration()
        let preferences = WKWebpagePreferences()
        // Disable JavaScript execution for security invariants
        preferences.allowsContentJavaScript = false
        config.defaultWebpagePreferences = preferences

        let webView = WKWebView(frame: .zero, configuration: config)
        return webView
    }

    public func updateNSView(_ nsView: WKWebView, context: Context) {
        let wrappedContent: String
        if isHTML {
            wrappedContent = """
            <!DOCTYPE html>
            <html>
            <head>
            <meta charset="utf-8">
            <meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; img-src data:;">
            <style>
              body { font-family: -apple-system, BlinkMacSystemFont, sans-serif; padding: 16px; margin: 0; color: #333; }
              @media (prefers-color-scheme: dark) {
                body { color: #eee; background: #1e1e1e; }
              }
            </style>
            </head>
            <body>
            \(content)
            </body>
            </html>
            """
        } else {
            // Render SVG directly with sandbox CSP
            wrappedContent = """
            <!DOCTYPE html>
            <html>
            <head>
            <meta charset="utf-8">
            <meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline';">
            <style>
              body { margin: 0; display: flex; justify-content: center; align-items: center; min-height: 100vh; background: transparent; }
              svg { max-width: 100%; height: auto; }
            </style>
            </head>
            <body>
            \(content)
            </body>
            </html>
            """
        }
        nsView.loadHTMLString(wrappedContent, baseURL: nil)
    }
}
