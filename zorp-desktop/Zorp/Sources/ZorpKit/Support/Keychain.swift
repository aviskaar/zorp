import Foundation
import Security

/// The API key, kept in the login keychain.
///
/// zorp keeps no key on disk: the settings file has no field for one and
/// `PUT /api/settings` holds it in memory only. A browser user exports
/// `ZORP_API_KEY` in a shell, which an app opened from the Dock never
/// sees, so the app keeps the key here and hands it to its in-process
/// server at launch. It never goes anywhere else.
public enum Keychain {
    static let service = "dev.zorp.Zorp"
    static let account = "api-key"

    public static func apiKey() -> String? {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
            kSecReturnData as String: true,
            kSecMatchLimit as String: kSecMatchLimitOne,
        ]
        var item: CFTypeRef?
        guard SecItemCopyMatching(query as CFDictionary, &item) == errSecSuccess,
              let data = item as? Data,
              let key = String(data: data, encoding: .utf8),
              !key.isEmpty else {
            return nil
        }
        return key
    }

    /// Store `key`, or remove the stored one when it is nil or empty.
    @discardableResult
    public static func setAPIKey(_ key: String?) -> Bool {
        let base: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
        ]
        SecItemDelete(base as CFDictionary)
        guard let key, !key.isEmpty else { return true }
        var add = base
        add[kSecValueData as String] = Data(key.utf8)
        add[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlock
        return SecItemAdd(add as CFDictionary, nil) == errSecSuccess
    }
}
