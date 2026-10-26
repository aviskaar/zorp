import Foundation

/// The words and the colour of one tool line, ported from
/// `web/src/activity-line.ts` so both surfaces describe a call the same
/// way. The server names a shell call `run_command(<command>)`; the line
/// shows a short phrase for it and keeps the verbatim command one click
/// away, because a phrase can be wrong.
public enum ActivityLine {
    public static let briefMax = 80

    public enum State: Equatable, Sendable {
        case running, ok, failed
    }

    public struct Call: Equatable, Sendable {
        public let tool: String
        public let command: String?
    }

    static let shellTools: Set<String> = ["run_command", "start_background_process"]

    static let failed: Set<String> = [
        "timed out", "cancelled", "failed", "error", "denied", "unknown tool",
        "blocked", "step limit", "repeated action", "verification failed",
    ]

    /// Split `run_command(pandoc a.html -o a.pdf)` into the tool and the
    /// command. A bare tool name has no command.
    public static func split(_ name: String) -> Call {
        guard let open = name.firstIndex(of: "("), open > name.startIndex, name.hasSuffix(")") else {
            return Call(tool: name, command: nil)
        }
        let tool = String(name[name.startIndex..<open])
        let allowed = CharacterSet.alphanumerics.union(CharacterSet(charactersIn: "_.:-"))
        guard tool.unicodeScalars.allSatisfy({ allowed.contains($0) }) else {
            return Call(tool: name, command: nil)
        }
        let command = String(name[name.index(after: open)..<name.index(before: name.endIndex)])
        return Call(tool: tool, command: command)
    }

    /// The colour for a finished call's status. `exited 0` is ok and any
    /// other exit code a failure; the agent loop's failure words are
    /// failures; everything else, including the other tools' own success
    /// summaries, is ok.
    public static func state(for status: String) -> State {
        let word = status.trimmingCharacters(in: .whitespacesAndNewlines)
        if word.hasPrefix("exited ") {
            let code = word.dropFirst("exited ".count)
            if Int(code) != nil { return code == "0" ? .ok : .failed }
        }
        if failed.contains(word) { return .failed }
        for prefix in ["error", "denied", "withheld"] where word.hasPrefix(prefix) {
            let rest = word.dropFirst(prefix.count)
            if rest.isEmpty || !(rest.first!.isLetter || rest.first!.isNumber) { return .failed }
        }
        if word.hasPrefix("0/"), word.hasSuffix(" blocks applied") { return .failed }
        return .ok
    }

    /// What the line says: the model's own phrase when it gave one,
    /// cleaned, otherwise a phrase computed from the command, otherwise
    /// the tool name.
    public static func headline(name: String, phrase: String?) -> (text: String, modelWritten: Bool) {
        let call = split(name)
        guard let command = call.command else { return (name, false) }
        if let own = clamp(phrase) { return (own, true) }
        let described = describe(command)
        return (shellTools.contains(call.tool) ? described : "\(call.tool) \(described)", false)
    }

    /// One line of model text, without control or bidirectional characters
    /// and without the quote marks and stars a model wraps around things.
    public static func clamp(_ raw: String?) -> String? {
        guard let raw else { return nil }
        let firstLine = raw.split(whereSeparator: { "\n\r\u{85}\u{2028}\u{2029}".contains($0) }).first.map(String.init) ?? ""
        let stripped = firstLine.unicodeScalars.filter { !isHidden($0) }
        var phrase = String(String.UnicodeScalarView(stripped))
            .components(separatedBy: .whitespacesAndNewlines).filter { !$0.isEmpty }.joined(separator: " ")
        let trim = CharacterSet(charactersIn: "\"'`*# ")
        phrase = phrase.trimmingCharacters(in: trim)
        return phrase.isEmpty ? nil : cap(phrase)
    }

    static func isHidden(_ s: Unicode.Scalar) -> Bool {
        if s.properties.generalCategory == .control { return true }
        switch s.value {
        case 0x200B...0x200F, 0x202A...0x202E, 0x2060...0x2064, 0x2066...0x2069, 0xFEFF: return true
        default: return false
        }
    }

    static func cap(_ text: String) -> String {
        guard text.count > briefMax else { return text }
        return String(text.prefix(briefMax - 1)).trimmingCharacters(in: .whitespaces) + "…"
    }

    static let phrases: [String: String] = {
        let table: [(String, String)] = [
            ("ls, tree, find, fd, exa, eza", "Listing files in {}"),
            ("cat, head, tail, less, more, bat", "Reading {}"),
            ("grep, rg, ag, ugrep", "Searching for \"{}\""),
            ("wc", "Counting lines in {}"),
            ("mkdir", "Creating directory {}"),
            ("touch", "Creating {}"),
            ("rm, rmdir", "Removing {}"),
            ("cp", "Copying {}"),
            ("mv", "Moving {}"),
            ("chmod, chown", "Changing permissions on {}"),
            ("sed, awk", "Editing text"),
            ("echo, printf", "Printing"),
            ("curl, wget", "Fetching {}"),
            ("pandoc", "Converting {}"),
            ("python, python3, node, bun, deno, ruby, perl, bash, sh, zsh", "Running a script"),
            ("make, cargo build, npm run build, pnpm run build, yarn run build, bun run build", "Building"),
            ("cargo check", "Checking"),
            ("cargo clippy", "Linting"),
            ("cargo fmt", "Formatting"),
            ("cargo run", "Running"),
            ("cargo add", "Adding a dependency"),
            ("cargo test, npm test, pnpm test, yarn test, bun test, pytest, jest, vitest, go test", "Running tests"),
            ("npm run, pnpm run, yarn run, bun run", "Running {}"),
            ("npm install, npm ci, npm add, pnpm install, pnpm add, yarn install, yarn add", "Installing packages"),
            ("bun install, bun add, pip install, pip3 install, uv pip install, uv add, uv sync", "Installing packages"),
            ("git status", "Checking git status"),
            ("git diff", "Showing changes"),
            ("git log", "Reading git history"),
            ("git add", "Staging changes"),
            ("git commit", "Committing"),
            ("git push", "Pushing"),
            ("git pull, git fetch", "Fetching from remote"),
            ("git checkout, git switch", "Switching branch"),
            ("git branch", "Listing branches"),
            ("git stash", "Stashing changes"),
            ("git clone", "Cloning"),
            ("docker build", "Building an image"),
            ("docker run", "Starting a container"),
            ("docker compose up", "Starting containers"),
            ("kill, pkill", "Stopping a process"),
            ("ps, top, lsof", "Listing processes"),
            ("which, whereis, type", "Locating a program"),
            ("env, printenv", "Reading the environment"),
            ("date, pwd, whoami, uname", "Checking the system"),
            ("cd", "Changing directory to {}"),
        ]
        var out: [String: String] = [:]
        for (programs, phrase) in table {
            for program in programs.components(separatedBy: ", ") { out[program] = phrase }
        }
        return out
    }()

    /// A short phrase for a shell command, read from its first segment.
    public static func describe(_ command: String) -> String {
        var rest = command.trimmingCharacters(in: .whitespacesAndNewlines)
        // A leading `cd somewhere &&` says where, not what.
        while let range = rest.range(of: #"^cd\s+("[^"]*"|'[^']*'|\S+)\s*(&&|;)\s*"#, options: .regularExpression) {
            rest.removeSubrange(range)
        }
        let (segment, more) = firstSegment(rest)
        var tokens = segment
        while let first = tokens.first, first.range(of: #"^[A-Za-z_][A-Za-z0-9_]*="#, options: .regularExpression) != nil {
            tokens.removeFirst()
        }
        guard let program = tokens.first else { return cap(rest) }
        var words = [program.components(separatedBy: "/").last ?? program]
        var target: String?
        var op: String?
        for token in tokens.dropFirst() {
            if let pending = op {
                if pending.contains(">") { target = target ?? token }
                op = nil
                continue
            }
            if token.range(of: #"^\d*(>>?|<|&>)$"#, options: .regularExpression) != nil {
                op = token
                continue
            }
            if token.hasPrefix("-") || token.hasPrefix("&")
                || token.range(of: #"^\d*[<>]"#, options: .regularExpression) != nil {
                continue
            }
            words.append(token)
        }
        var phrase = "Running " + words.prefix(2).map(unquote).joined(separator: " ")
        for width in stride(from: min(3, words.count), through: 1, by: -1) {
            if let found = phrases[words.prefix(width).joined(separator: " ")] {
                phrase = fill(found, width < words.count ? words[width] : nil)
                break
            }
        }
        if let target, ["echo", "printf", "cat"].contains(words[0]) {
            phrase = "Writing " + unquote(target)
        }
        if more { phrase += " …" }
        return cap(phrase)
    }

    static func fill(_ phrase: String, _ object: String?) -> String {
        guard let object else {
            return phrase.replacingOccurrences(of: #"(?: (?:in|for|on|to))? "?\{\}"?$"#, with: "", options: .regularExpression)
        }
        return phrase.replacingOccurrences(of: "{}", with: unquote(object))
    }

    static func unquote(_ token: String) -> String {
        for q in ["\"", "'"] where token.count >= 2 && token.hasPrefix(q) && token.hasSuffix(q) {
            return String(token.dropFirst().dropLast())
        }
        return token
    }

    static func firstSegment(_ command: String) -> ([String], Bool) {
        var tokens: [String] = []
        var current = ""
        var quote: Character?
        var more = false
        let chars = Array(command)
        var index = 0
        func push() {
            if !current.isEmpty { tokens.append(current) }
            current = ""
        }
        while index < chars.count {
            let c = chars[index]
            if let q = quote {
                current.append(c)
                if c == q { quote = nil }
            } else if c == "\"" || c == "'" {
                quote = c
                current.append(c)
            } else if c == "\\" && index + 1 < chars.count {
                current.append(c)
                current.append(chars[index + 1])
                index += 1
            } else if c == "|" || c == ";" || c == "\n" || (c == "&" && index + 1 < chars.count && chars[index + 1] == "&") {
                more = true
                break
            } else if c.isWhitespace {
                push()
            } else {
                current.append(c)
            }
            index += 1
        }
        push()
        return (tokens, more)
    }
}
