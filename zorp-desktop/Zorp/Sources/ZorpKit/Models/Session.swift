import Foundation

public struct SessionSummary: Codable, Identifiable, Hashable, Sendable {
    public let id: String
    public let title: String
    public let status: String
    public let projectId: String?
    public let agent: String?
    public let createdAt: Int64?
    public let updatedAt: Int64?

    enum CodingKeys: String, CodingKey {
        case id
        case title
        case status
        case projectId = "project_id"
        case agent
        case createdAt = "created_at"
        case updatedAt = "updated_at"
    }

    public init(id: String, title: String, status: String, projectId: String? = nil, agent: String? = nil, createdAt: Int64? = nil, updatedAt: Int64? = nil) {
        self.id = id
        self.title = title
        self.status = status
        self.projectId = projectId
        self.agent = agent
        self.createdAt = createdAt
        self.updatedAt = updatedAt
    }
}

public struct ProjectItem: Codable, Identifiable, Hashable, Sendable {
    public let id: String
    public let name: String

    public init(id: String, name: String) {
        self.id = id
        self.name = name
    }
}

public struct ArtifactItem: Codable, Identifiable, Hashable, Sendable {
    public var id: String { path }
    public let path: String
    public let isDir: Bool
    public let sizeBytes: Int64?

    enum CodingKeys: String, CodingKey {
        case path
        case isDir = "is_dir"
        case sizeBytes = "size_bytes"
    }

    public init(path: String, isDir: Bool = false, sizeBytes: Int64? = nil) {
        self.path = path
        self.isDir = isDir
        self.sizeBytes = sizeBytes
    }
}
